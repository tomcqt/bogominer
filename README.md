# bogominer.

bogominer is the official desktop bogosorting client for [swapjs' bogostream](https://bogo.swapjs.dev).

we gladly accept contributions! if you have anything you want to change, please create a pr!

## gpu mining (beta)

bogominer can offload mining to [bogo-turbo](https://github.com/Mafiosoweb1/bogo-turbo), an external optimized CUDA worker (NVIDIA RTX 20xx–50xx). the prebuilt worker (~2 MB, pinned to a fixed bogo-turbo commit) is downloaded automatically **on launch** if it isn't already present, right next to the bogominer executable (windows; falls back to the app data dir if the exe folder isn't writable, e.g. an install under Program Files). on linux build bogo-turbo from source and set its path in settings. then just flip **GPU acceleration** in settings and start mining. a worker set via "gpu worker path" takes precedence and is never overwritten by the auto-download.

bogominer launches the worker with your saved account credentials and feeds its protocol log into the regular dashboard stats. while enabled, gpu mining replaces cpu mining — the server allows a single connection per account.

## updates

### 2026-06-15
- the gpu worker now always tracks the latest `bogo-turbo` build instead of a pinned commit
- bogominer checks for a newer worker on launch and auto-updates it in place if a newer one is available
- fixed a bug where gpu sort would stop at 1000T
