//! Builds the vendored WavPack library (third_party/wavpack) for every target, so Linux and
//! Android decode with the same version and no system package is needed.

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/wavpack");
    let sources = [
        "common_utils",
        "decorr_utils",
        "entropy_utils",
        "extra1",
        "extra2",
        "open_utils",
        "open_filename",
        "open_legacy",
        "open_raw",
        "pack",
        "pack_dns",
        "pack_floats",
        "pack_utils",
        "read_words",
        "tags",
        "tag_utils",
        "unpack",
        "unpack_floats",
        "unpack_seek",
        "unpack_utils",
        "write_words",
        "pack_dsd",
        "unpack_dsd",
    ];
    let mut build = cc::Build::new();
    build
        .include(root.join("include"))
        .define("ENABLE_DSD", None)
        .warnings(false);
    for s in sources {
        build.file(root.join("src").join(format!("{s}.c")));
    }
    build.compile("wavpack");
    println!("cargo:rerun-if-changed={}", root.display());
}
