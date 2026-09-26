//! Types the example's messages from `locales/*.ftl`, and embeds its
//! resources (`rustnative.toml`).

fn main() {
    rustnative_build::compile_messages();
    rustnative_build::embed_resources();
}
