use super::*;

#[test]
fn map_returns_the_results_in_the_order_of_the_items_for_any_thread_count() {
    let items: Vec<u64> = (0..500).collect();
    let expected: Vec<u64> = items.iter().map(|x| x * x).collect();
    for threads in [1, 2, 3, 16, 64] {
        assert_eq!(
            map(&items, threads, |x| x * x),
            expected,
            "{threads} threads"
        );
    }
}

#[test]
fn map_of_no_items_is_empty() {
    let items: Vec<u8> = Vec::new();
    assert_eq!(map(&items, 8, |x| *x), Vec::<u8>::new());
}

#[test]
fn an_automatic_thread_count_is_at_least_one_and_at_most_the_cap() {
    let count = threads(0);
    assert!((1..=MAX_THREADS).contains(&count));
}

#[test]
fn a_requested_thread_count_is_used_as_given() {
    assert_eq!(threads(3), 3);
}

#[test]
fn an_exact_budget_refuses_exactly_the_item_past_the_limit() {
    let budget = SharedBudget::new(3, 1);
    let mut holder = budget.holder();
    assert!(holder.take() && holder.take() && holder.take());
    assert!(!holder.take());
    assert_eq!(holder.count(), 3);
}

#[test]
fn dropping_a_holder_gives_its_reservation_back() {
    let budget = SharedBudget::new(10, 4);
    {
        let mut holder = budget.holder();
        assert!(holder.take());
    }
    assert_eq!(budget.used(), 0);
}

#[test]
fn keeping_part_of_a_reservation_gives_the_rest_back() {
    let budget = SharedBudget::new(10, 4);
    let mut holder = budget.holder();
    for _ in 0..5 {
        assert!(holder.take());
    }
    holder.keep(2);
    assert_eq!(budget.used(), 2);
}

#[test]
fn a_chunked_budget_never_lets_the_total_pass_the_limit() {
    let budget = SharedBudget::new(1000, 64);
    let taken: Vec<usize> = map(&[(); 8], 8, |()| {
        let mut holder = budget.holder();
        while holder.take() {}
        let count = holder.count();
        holder.keep(count);
        count
    });
    assert!(taken.iter().sum::<usize>() <= 1000);
}
