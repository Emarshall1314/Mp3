# WavPack 5.9.0 (vendored)

The C library sources from <https://github.com/dbry/WavPack>, tag `5.9.0`
(commit 5803634a030e2a11dba602ba057b89cc34486c67). The files are unmodified: only the library
sources are copied, without the programs, plugins or assembly. Licence: BSD-3-Clause (`COPYING`).

`core/decode/build.rs` compiles these sources with the `cc` crate on every target, with `ENABLE_DSD`
and no legacy (< 4.0) support. Android's NDK has no libwavpack, and building it ourselves keeps
Linux and Android on the same version (ARCHITECTURE D27).
