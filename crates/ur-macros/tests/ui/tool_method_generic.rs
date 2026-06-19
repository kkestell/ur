use ur_macros::tools;

struct Tools;

#[tools]
impl Tools {
    #[ur::tool]
    async fn ping<T>(&self, x: T) -> i64 {
        let _ = x;
        1
    }
}

fn main() {}
