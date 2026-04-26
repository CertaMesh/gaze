struct RestoreState {
    token_count: usize,
}

fn main() {
    let state = RestoreState { token_count: 0 };
    let _ = state.token_count;
}
