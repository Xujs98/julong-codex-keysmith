# macOS WebKit Startup Patch

This vendored `tauri-runtime-wry` 2.11.4 contains one local compatibility
change. On Intel macOS 15, an installed application can crash inside
CoreFoundation when Wry probes `NSBundle` for `com.apple.WebKit` during Tauri
startup.

macOS includes WebKit as a system component, so the local patch marks the
runtime as available without performing that unsafe startup probe. Other
platforms keep the upstream runtime detection behavior.
