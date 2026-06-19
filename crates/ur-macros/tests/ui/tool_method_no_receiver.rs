use ur_macros::tools;

struct Tools;

#[tools]
impl Tools {
    #[ur::tool]
    async fn ping(x: i64) -> i64 {
        x
    }
}

fn main() {}
