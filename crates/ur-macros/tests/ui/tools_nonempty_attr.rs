use ur_macros::tools;

struct Tools;

#[tools(name = "x")]
impl Tools {
    #[ur::tool]
    async fn ping(&self) -> i64 {
        1
    }
}

fn main() {}
