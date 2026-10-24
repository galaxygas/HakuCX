# Third-party software shipped with HakuCX

HakuCX itself is under the [Mozilla Public License 2.0](../LICENSE). The Windows packages also
contain these separately licensed programs, next to `hakucx.exe`:

| Component | Files | License | Source |
|---|---|---|---|
| Servo (built into `hakucx.exe`) | `hakucx.exe` | MPL-2.0 | https://github.com/servo/servo |
| ANGLE (OpenGL ES on Direct3D) | `libEGL.dll`, `libGLESv2.dll` | BSD-3-Clause | https://chromium.googlesource.com/angle/angle |
| Microsoft Visual C++ runtime | `msvcp140*.dll`, `vcruntime140*.dll`, `concrt140.dll` | Microsoft Visual C++ redistributable terms (app-local deployment) | https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files |
| GStreamer 1.28 and the libraries its plugins use (GLib, FFmpeg's libavcodec/libavformat/libavutil, libvpx (inside `gstvpx.dll`), libogg, libvorbis, libopus, libtheora, libnice, OpenSSL, ...) | `gst*.dll` and the other DLLs listed in `gstreamer-files.txt` at build time | GStreamer: LGPL-2.1-or-later. Each library's license is in `licenses\gstreamer\<name>\` in the installed folder. | https://gstreamer.freedesktop.org/src/ and https://gstreamer.freedesktop.org/data/pkg/windows/ |

## GStreamer and the LGPL

GStreamer plays audio and video in web pages (Servo's media backend). HakuCX uses the official
MSVC binaries from the GStreamer project, unmodified, as separate DLLs that `hakucx.exe` loads
at run time. You may replace them with your own build of the same GStreamer version: put your
DLLs in the HakuCX folder in place of the shipped ones. The source code of every library is
available from the links above (the GStreamer project publishes the exact sources used for its
Windows binaries, and the recipes that built them, in its `cerbero` repository).

GStreamer's own license notes list its FFmpeg build under LGPL-2.1-or-later. The GStreamer
binaries also contain GPL components (x264, x265, DVD libraries); HakuCX does not ship those:
only the plugins Servo loads and the libraries they import are included.
