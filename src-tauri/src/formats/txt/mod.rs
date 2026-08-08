mod decoder;
mod encoder;
mod line_endings;

pub use decoder::decode_text;
pub use encoder::encode_text;
pub use line_endings::analyze_and_normalize;
