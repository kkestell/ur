use ur_macros::tools;

struct Tools;

#[tools]
impl Tools {
    #[ur::tool]
    async fn ping(&mut self) -> i64 {
        1
    }
}

fn main() {}
