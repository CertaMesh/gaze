use gaze::types::RawRow;

fn assert_serialize<T: serde::Serialize>() {}

fn main() {
    assert_serialize::<RawRow>();
}
