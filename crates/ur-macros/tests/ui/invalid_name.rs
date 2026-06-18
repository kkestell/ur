use ur_macros::tool;

#[tool(name = "bad name")]
async fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn main() {}
