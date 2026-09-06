use std::path::Path;

mod share;
mod source;
#[cfg(test)]
mod tests;

pub use share::share;
pub use source::query_sources;

fn basename_or_self(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}
