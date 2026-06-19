use ur_macros::tools;

struct Tools;

#[tools]
impl Tools {
    #[ur::tool]
    async unsafe fn ping(&self) -> i64 {
        1
    }
}

fn main() {}
