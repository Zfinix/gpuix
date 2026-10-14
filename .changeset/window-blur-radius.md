---
'@gpuix/native': minor
'@gpuix/react': minor
---

Added `setWindowBlurRadius(radius)`, which changes how strongly a `"blurred"` window blurs the desktop on macOS while the app runs. `null` restores the system blur. It does nothing on other platforms. See "Window chrome" in the README.
