fn main() {
    glib_build_tools::compile_resources(
        &["data"],
        "data/com.linofish.MomentStays.gresource.xml",
        "momentstays.gresource",
    );
    println!("cargo:rerun-if-env-changed=MOMENTSTAYS_LOCALEDIR");
}
