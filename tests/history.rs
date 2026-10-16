use plugii::history::{History, Sample};

fn at(t_ms: u64) -> Sample {
    Sample {
        t_ms,
        pd_w: Some(t_ms as f32),
        ..Sample::default()
    }
}

fn times(samples: impl Iterator<Item = Sample>) -> Vec<u64> {
    samples.map(|s| s.t_ms).collect()
}

#[test]
fn empty() {
    let h: History<Sample> = History::new(3);
    assert_eq!(h.len(), 0);
    assert!(h.is_empty());
    assert_eq!(h.capacity(), 3);
    assert_eq!(h.latest(), None);
    assert_eq!(h.since(0).count(), 0);
}

#[test]
fn keeps_samples_in_order_up_to_capacity() {
    let mut h = History::new(3);
    for t in [10, 20, 30] {
        h.push(at(t));
    }
    assert_eq!(h.len(), 3);
    assert_eq!(times(h.iter().copied()), [10, 20, 30]);
    assert_eq!(h.latest(), Some(&at(30)));
}

#[test]
fn evicts_the_oldest_when_full() {
    let mut h = History::new(3);
    for t in [10, 20, 30, 40, 50] {
        h.push(at(t));
    }
    assert_eq!(h.len(), 3);
    assert_eq!(times(h.iter().copied()), [30, 40, 50]);
    assert_eq!(h.latest(), Some(&at(50)));
}

#[test]
fn since_is_inclusive() {
    let mut h = History::new(10);
    for t in [10, 20, 30, 40] {
        h.push(at(t));
    }
    assert_eq!(times(h.since(0).copied()), [10, 20, 30, 40]);
    assert_eq!(times(h.since(20).copied()), [20, 30, 40]);
    assert_eq!(times(h.since(21).copied()), [30, 40]);
    assert_eq!(times(h.since(41).copied()), Vec::<u64>::new());
}

#[test]
fn since_after_wrap_around() {
    let mut h = History::new(3);
    for t in [10, 20, 30, 40, 50] {
        h.push(at(t));
    }
    assert_eq!(times(h.since(15).copied()), [30, 40, 50]);
    assert_eq!(times(h.since(45).copied()), [50]);
}

#[test]
#[should_panic(expected = "capacity")]
fn zero_capacity_is_a_bug() {
    let _: History<Sample> = History::new(0);
}
