use ur_macros::tool;

#[tool]
async fn add<T>(a: T) -> i64 {
    let _ = a;
    0
}

fn main() {}
