// Small numeric helper functions, unrelated to authentication.

fn factorial(n: u64) -> u64 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}

fn fibonacci(n: u64) -> u64 {
    if n < 2 {
        n
    } else {
        fibonacci(n - 1) + fibonacci(n - 2)
    }
}

fn sum_of_squares(values: &[u64]) -> u64 {
    values.iter().map(|v| v * v).sum()
}
