use crate::backend::miner::Miner;
use crate::backend::solver::{RangeResult, N};
use pollster::FutureExt as _;

const SHADER: &str = include_str!("kernel.wgsl");

const BLOCK_RESULT_BYTES: usize = 128;

const THREADS_PER_BLOCK: u32 = 256;
const DEFAULT_BLOCKS: u32 = 1024;
const GPU_CHUNK_SIZE: u64 = 256 * 1024 * 1024;

pub struct SelectedAdapter {
    pub adapter: wgpu::Adapter,
    pub label: String,
}

pub struct GlobalGpuMiner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    params_buf: wgpu::Buffer,
    results_buf: wgpu::Buffer,
    staging_buf: wgpu::Buffer,
    blocks: u32,
    label: String,
    score_history: [u32; 32],
    history_pos: usize,
    history_count: usize,
}

impl GlobalGpuMiner {
    pub fn new(selected: SelectedAdapter) -> Result<Self, String> {
        let SelectedAdapter { adapter, label } = selected;

        if !adapter.features().contains(wgpu::Features::SHADER_INT64) {
            return Err(format!(
                "{} does not support SHADER_INT64 (required by the gpu kernel)",
                label
            ));
        }
        eprintln!("[gpu] initialising backend on {}", label);

        let (device, queue) = async {
            adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("bogominer-gpu"),
                        required_features: wgpu::Features::SHADER_INT64,
                        required_limits: wgpu::Limits::default(),
                        memory_hints: wgpu::MemoryHints::Performance,
                    },
                    None,
                )
                .await
        }
        .block_on()
        .map_err(|e| format!("request_device failed: {}", e))?;

        device.on_uncaptured_error(Box::new(|e| {
            eprintln!("[gpu] uncaptured device error: {:?}", e);
        }));

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bogo_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bogo_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bogo_layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("bogo_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        let blocks = DEFAULT_BLOCKS;
        let n = blocks as usize;

        // params: 7 x u32 = 28 bytes (6 seed/range words + threshold)
        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 28,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let results_size = (n * BLOCK_RESULT_BYTES) as u64;
        let results_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("results"),
            size: results_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: results_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        eprintln!(
            "[gpu] pipeline ready: blocks={} threads_per_block={} chunk={}",
            blocks, THREADS_PER_BLOCK, GPU_CHUNK_SIZE
        );

        Ok(Self {
            device,
            queue,
            pipeline,
            bgl,
            params_buf,
            results_buf,
            staging_buf,
            blocks,
            label,
            score_history: [0u32; 32],
            history_pos: 0,
            history_count: 0,
        })
    }

    // moving average of recent best scores, mirrors cuda/hip host logic
    fn record_score(&mut self, score: u32) {
        if score == 0 {
            return;
        }
        self.score_history[self.history_pos] = score;
        self.history_pos = (self.history_pos + 1) % 32;
        if self.history_count < 32 {
            self.history_count += 1;
        }
    }

    fn min_threshold(&self) -> u32 {
        if self.history_count < 8 {
            return 0;
        }
        let sum: u32 = self.score_history[..self.history_count].iter().sum();
        (sum / self.history_count as u32).saturating_sub(1)
    }
}

impl Miner for GlobalGpuMiner {
    fn name(&self) -> String {
        format!("gpu/global ({})", self.label)
    }

    fn compute_range(&mut self, seed: u64, lo: u64, hi: u64, threshold: i32) -> RangeResult {
        let eff_threshold = threshold.max(self.min_threshold() as i32);
        let params_data: [u32; 7] = [
            seed as u32,
            (seed >> 32) as u32,
            lo as u32,
            (lo >> 32) as u32,
            hi as u32,
            (hi >> 32) as u32,
            eff_threshold as u32,
        ];
        self.queue
            .write_buffer(&self.params_buf, 0, bytemuck::cast_slice(&params_data));

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.results_buf.as_entire_binding(),
                },
            ],
        });

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(self.blocks, 1, 1);
        }
        enc.copy_buffer_to_buffer(
            &self.results_buf,
            0,
            &self.staging_buf,
            0,
            (self.blocks as usize * BLOCK_RESULT_BYTES) as u64,
        );
        self.queue.submit(Some(enc.finish()));

        // map staging and block until the gpu is done
        let slice = self.staging_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);
        if let Err(e) = rx.recv().expect("map_async channel") {
            eprintln!("[gpu] buffer map failed: {:?}", e);
            return RangeResult {
                count: hi - lo,
                best_correct: -1,
                best_arr: [0u8; N],
                best_index: lo,
            };
        }

        let data = slice.get_mapped_range();
        let result = find_winner(&data, self.blocks as usize, lo, hi);
        drop(data);
        self.staging_buf.unmap();
        if result.best_correct > 0 {
            self.record_score(result.best_correct as u32);
        }
        result
    }

    fn chunk_size(&self) -> u64 {
        GPU_CHUNK_SIZE
    }
}

// scan every workgroups BlockResult for the global best
// ties broken by lowest index to match the kernels reduction and servers replay
fn find_winner(data: &[u8], blocks: usize, lo: u64, hi: u64) -> RangeResult {
    let mut best_correct: i32 = -1;
    let mut best_arr = [0u8; N];
    let mut best_index = lo;

    for b in 0..blocks {
        let base = b * BLOCK_RESULT_BYTES;
        let score = i32::from_le_bytes(data[base..base + 4].try_into().unwrap());
        if score < best_correct {
            continue;
        }

        let idx_lo = u32::from_le_bytes(data[base + 8..base + 12].try_into().unwrap());
        let idx_hi = u32::from_le_bytes(data[base + 12..base + 16].try_into().unwrap());
        let index = (idx_lo as u64) | ((idx_hi as u64) << 32);

        if score == best_correct && index >= best_index {
            continue;
        }

        let arr_start = base + 16;
        let mut arr = [0u8; N];
        for (i, j) in arr.iter_mut().enumerate().take(N) {
            let off = arr_start + i * 4;
            *j = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as u8;
        }

        best_correct = score;
        best_index = index;
        best_arr = arr;
    }

    RangeResult {
        count: hi - lo,
        best_correct,
        best_arr,
        best_index,
    }
}
