use proptest::prelude::*;
use scriptum_daemon::engine::ydoc::YDoc;

const TEXT_KEY: &str = "content";
const OPS_PER_RUN: usize = 10_000;

#[derive(Debug, Clone)]
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        self.state
    }

    fn next_usize(&mut self, upper_exclusive: usize) -> usize {
        if upper_exclusive == 0 {
            return 0;
        }
        (self.next_u64() as usize) % upper_exclusive
    }
}

fn sync_docs(source: &YDoc, target: &YDoc) {
    let target_sv = target.encode_state_vector();
    let diff = source.encode_diff(&target_sv).expect("state vector should decode");
    target.apply_update(&diff).expect("diff should apply");
}

fn random_edge_sync(docs: &[YDoc], rng: &mut Lcg) {
    if docs.len() < 2 {
        return;
    }

    let from = rng.next_usize(docs.len());
    let mut to = rng.next_usize(docs.len());
    if to == from {
        to = (to + 1) % docs.len();
    }
    sync_docs(&docs[from], &docs[to]);
}

fn settle_all(docs: &[YDoc]) {
    for _ in 0..3 {
        for from in 0..docs.len() {
            for to in 0..docs.len() {
                if from == to {
                    continue;
                }
                sync_docs(&docs[from], &docs[to]);
            }
        }
    }
}

fn random_insert_text(rng: &mut Lcg, min_len: usize, max_len: usize) -> String {
    let span = max_len.saturating_sub(min_len).saturating_add(1);
    let len = min_len + rng.next_usize(span);
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let ch = match rng.next_usize(40) {
            0..=25 => char::from(b'a' + rng.next_usize(26) as u8),
            26..=35 => char::from(b'0' + rng.next_usize(10) as u8),
            36 => ' ',
            37 => '\n',
            38 => '-',
            _ => '_',
        };
        out.push(ch);
    }
    out
}

fn apply_random_edit(doc: &YDoc, rng: &mut Lcg, max_insert_len: usize) {
    let len = doc.text_len(TEXT_KEY) as usize;

    if len == 0 || rng.next_usize(3) == 0 {
        let index = rng.next_usize(len.saturating_add(1)) as u32;
        let text = random_insert_text(rng, 1, max_insert_len.max(1));
        doc.insert_text(TEXT_KEY, index, &text);
        return;
    }

    match rng.next_usize(2) {
        0 => {
            let start = rng.next_usize(len);
            let max_delete = len - start;
            let delete_len = 1 + rng.next_usize(max_delete);
            doc.remove_text(TEXT_KEY, start as u32, delete_len as u32);
        }
        _ => {
            let start = rng.next_usize(len);
            let max_replace = len - start;
            let replace_len = 1 + rng.next_usize(max_replace);
            let text = random_insert_text(rng, 1, max_insert_len.max(1));
            doc.replace_text(TEXT_KEY, start as u32, replace_len as u32, &text);
        }
    }
}

fn apply_concurrent_same_position_insert(docs: &[YDoc], rng: &mut Lcg) {
    if docs.len() < 2 {
        return;
    }

    let a = rng.next_usize(docs.len());
    let mut b = rng.next_usize(docs.len());
    if b == a {
        b = (b + 1) % docs.len();
    }

    // Bring both replicas to the same frontier, then edit concurrently.
    sync_docs(&docs[a], &docs[b]);
    sync_docs(&docs[b], &docs[a]);

    let len = docs[a].text_len(TEXT_KEY) as usize;
    let index = rng.next_usize(len.saturating_add(1)) as u32;
    let insert_a = random_insert_text(rng, 1, 10);
    let insert_b = random_insert_text(rng, 1, 10);
    docs[a].insert_text(TEXT_KEY, index, &insert_a);
    docs[b].insert_text(TEXT_KEY, index, &insert_b);
}

fn apply_rapid_interleaving(docs: &[YDoc], rng: &mut Lcg) {
    let rounds = 3 + rng.next_usize(5);
    for _ in 0..rounds {
        let actor = rng.next_usize(docs.len());
        apply_random_edit(&docs[actor], rng, 8);
        random_edge_sync(docs, rng);
    }
}

fn seed_large_document(doc: &YDoc) {
    let mut markdown = String::with_capacity(150_000);
    for i in 0..2_200 {
        markdown.push_str("## Section ");
        markdown.push_str(&i.to_string());
        markdown.push('\n');
        markdown.push_str("Status: open\n");
        markdown.push_str("- bullet alpha\n");
        markdown.push_str("- bullet beta\n");
        markdown.push_str("Body: lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\n");
    }
    assert!(markdown.len() > 100_000, "seeded markdown should exceed 100KB");
    doc.insert_text(TEXT_KEY, 0, &markdown);
}

fn run_randomized_convergence(seed: u64, clients: usize, ops: usize, include_large_seed: bool) {
    assert!(clients >= 2, "at least two replicas are required");

    let docs = (0..clients).map(|idx| YDoc::with_client_id((idx + 1) as u64)).collect::<Vec<_>>();
    let mut rng = Lcg::new(seed);

    if include_large_seed {
        seed_large_document(&docs[0]);
        settle_all(&docs);
    }

    // Ensure each required behavior is exercised in every run.
    apply_concurrent_same_position_insert(&docs, &mut rng);
    apply_rapid_interleaving(&docs, &mut rng);

    for _ in 0..ops {
        match rng.next_usize(5) {
            0..=2 => {
                let actor = rng.next_usize(clients);
                apply_random_edit(&docs[actor], &mut rng, 16);
            }
            3 => apply_concurrent_same_position_insert(&docs, &mut rng),
            _ => {
                let actor = rng.next_usize(clients);
                apply_random_edit(&docs[actor], &mut rng, 12);
                random_edge_sync(&docs, &mut rng);
            }
        }

        if rng.next_usize(4) == 0 {
            random_edge_sync(&docs, &mut rng);
        }

        // Occasional short interleaving bursts emulate rapid concurrent activity.
        if rng.next_usize(37) == 0 {
            apply_rapid_interleaving(&docs, &mut rng);
        }
    }

    settle_all(&docs);

    let expected = docs[0].get_text_string(TEXT_KEY);
    for (idx, doc) in docs.iter().enumerate().skip(1) {
        let actual = doc.get_text_string(TEXT_KEY);
        assert_eq!(
            actual, expected,
            "convergence mismatch for seed={seed}, clients={clients}, ops={ops}, client={idx}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1,
        max_shrink_iters: 64,
        .. ProptestConfig::default()
    })]

    #[test]
    fn crdt_converges_with_10k_randomized_ops(seed in any::<u64>(), clients in 3usize..6) {
        run_randomized_convergence(seed, clients, OPS_PER_RUN, false);
    }

    #[test]
    fn crdt_converges_from_large_documents(seed in any::<u64>()) {
        run_randomized_convergence(seed ^ 0xC0FF_EE11, 4, 1_200, true);
    }
}
