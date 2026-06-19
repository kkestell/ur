use ur_macros::tools;

trait Other {}

struct Tools;

#[tools]
impl Other for Tools {
    #[ur::tool]
    async fn ping(&self) -> i64 {
        1
    }
}

fn main() {}
