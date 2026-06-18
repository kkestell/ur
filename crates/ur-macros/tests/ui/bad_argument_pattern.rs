use ur_macros::tool;

#[tool]
async fn add((a, b): (i64, i64)) -> i64 {
    a + b
}

fn main() {}
