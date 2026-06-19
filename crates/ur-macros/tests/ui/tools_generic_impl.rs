use ur_macros::tools;

struct Tools<T>(T);

#[tools]
impl<T> Tools<T> {
    #[ur::tool]
    async fn ping(&self) -> i64 {
        1
    }
}

fn main() {}
