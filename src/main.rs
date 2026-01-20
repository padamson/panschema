fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_cli() {
        // Placeholder test to ensure nextest has something to run
        assert_eq!(2 + 2, 4);
    }
}
