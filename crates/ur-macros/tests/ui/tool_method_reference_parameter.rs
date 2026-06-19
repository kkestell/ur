use ur_macros::tools;

struct Tools;

#[tools]
impl Tools {
    #[ur::tool]
    async fn lookup(&self, key: &str) -> i64 {
        let _ = key;
        0
    }
}

fn main() {}
