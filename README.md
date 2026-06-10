# bogominer.

bogominer is the official desktop bogosorting client for [swapjs' bogostream](https://bogo.swapjs.dev).

we gladly accept contributions! if you have anything you want to change, please create a pr!

## gpu mining (beta)

bogominer can offload mining to [bogo-turbo](https://github.com/Mafiosoweb1/bogo-turbo), an external optimized CUDA worker (NVIDIA RTX 20xx–50xx). just flip **GPU acceleration** in settings and start mining — the prebuilt worker (~2 MB, pinned to a fixed bogo-turbo commit) is downloaded automatically into the app data dir on first use (windows; on linux build bogo-turbo from source and set its path in settings). a worker placed next to the bogominer binary or set via "gpu worker path" takes precedence.

bogominer launches the worker with your saved account credentials and feeds its protocol log into the regular dashboard stats. while enabled, gpu mining replaces cpu mining — the server allows a single connection per account.
