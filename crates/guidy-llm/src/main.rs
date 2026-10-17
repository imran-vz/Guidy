//! Built-in llama.cpp worker. See docs/MEETING_NOTES_ARCH.md.
fn main() {
    let backend = llama_cpp_2::llama_backend::LlamaBackend::init().unwrap();
    drop(backend);
}
