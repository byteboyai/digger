use similar::{Algorithm, capture_diff_slices};
fn main() {
    let base = ["一", "，", "中", "等", "的", "主"];
    let ours = ["一", "【", "外", "，", "的", "声", "情", "】", "的", "主"];
    for op in capture_diff_slices(Algorithm::Myers, &base, &ours) { println!("{op:?}"); }
}
