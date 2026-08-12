fn main() {
    slint_build::compile("../../ui/readloom.slint").expect("compile Readloom Slint UI");
    #[cfg(windows)]
    embed_resource::compile("readloom.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("embed Readloom Windows icon");
}
