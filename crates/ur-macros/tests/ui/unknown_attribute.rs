use ur_macros::tool;

#[tool(nonsense = "x")]
async fn add(a: i64) -> i64 {
    a
}

fn main() {}
