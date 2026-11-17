// SPDX-License-Identifier: AGPL-3.0-or-later
//! The evaluator over plain values (docs/specs/m1/04-unlock-and-completion-rules.md T1 to T8).

use std::time::{Duration, UNIX_EPOCH};

use proptest::prelude::*;

use super::*;

/// 10:00:00 on 2025-11-13, UTC.
fn ten() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_028_000)
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn module(n: u128, items: Vec<ItemRules>) -> ModuleRules {
    ModuleRules {
        id: id(n),
        unlock_at: None,
        prerequisite_ids: Vec::new(),
        require_sequential: false,
        completion_rule: CompletionRule::All,
        items,
    }
}

fn required(n: u128) -> ItemRules {
    ItemRules {
        id: id(n),
        requirement: Some(Requirement::View),
    }
}

fn plain(n: u128) -> ItemRules {
    ItemRules {
        id: id(n),
        requirement: None,
    }
}

fn requiring(mut module: ModuleRules, prerequisites: &[u128]) -> ModuleRules {
    module.prerequisite_ids = prerequisites.iter().map(|&n| id(n)).collect();
    module
}

fn course(modules: Vec<ModuleRules>) -> CourseRules {
    CourseRules { modules }
}

fn done(items: &[u128]) -> HashSet<Uuid> {
    items.iter().map(|&n| id(n)).collect()
}

fn module_state(evaluation: &Evaluation, n: u128) -> &State {
    &evaluation.module(id(n)).expect("the module").state
}

fn item_state(evaluation: &Evaluation, n: u128) -> &State {
    &evaluation.item(id(n)).expect("the item").state
}

fn prerequisites(modules: &[u128]) -> LockReason {
    LockReason::Prerequisites {
        module_ids: modules.iter().map(|&n| id(n)).collect(),
    }
}

fn locked(reasons: Vec<LockReason>) -> State {
    State::Locked(reasons)
}

#[test]
fn t1_an_unlock_date_is_compared_with_the_request_time_inclusively() {
    let mut dated = module(1, vec![required(10)]);
    dated.unlock_at = Some(ten());
    let rules = course(vec![dated]);
    let cases = [
        (ten() - Duration::from_secs(1), true),
        (ten() - Duration::from_micros(1), true),
        (ten(), false),
        (ten() + Duration::from_secs(1), false),
    ];
    for (now, is_locked) in cases {
        let evaluation = evaluate(&rules, &done(&[]), now);
        let expected = if is_locked {
            locked(vec![LockReason::UnlockAt { at: ten() }])
        } else {
            State::Available
        };
        assert_eq!(module_state(&evaluation, 1), &expected, "{now:?}");
        assert_eq!(
            item_state(&evaluation, 10).is_locked(),
            is_locked,
            "{now:?}"
        );
    }
}

#[test]
fn t2_prerequisites_list_the_unsatisfied_ones_in_course_order_with_the_date() {
    let a = module(1, vec![required(10)]);
    let b = module(2, vec![required(20)]);
    // Named B first: the reason lists course order, not the stored order.
    let c = requiring(module(3, vec![required(30)]), &[2, 1]);
    let mut dated_c = c.clone();
    dated_c.unlock_at = Some(ten() + Duration::from_secs(3600));
    let open = course(vec![a.clone(), b.clone(), c]);
    let dated = course(vec![a, b, dated_c]);
    let cases: [(&CourseRules, &[u128], State); 5] = [
        (&open, &[], locked(vec![prerequisites(&[1, 2])])),
        (&open, &[10], locked(vec![prerequisites(&[2])])),
        (&open, &[20], locked(vec![prerequisites(&[1])])),
        (&open, &[10, 20], State::Available),
        (
            &dated,
            &[10],
            locked(vec![
                LockReason::UnlockAt {
                    at: ten() + Duration::from_secs(3600),
                },
                prerequisites(&[2]),
            ]),
        ),
    ];
    for (rules, completed, expected) in cases {
        let evaluation = evaluate(rules, &done(completed), ten());
        assert_eq!(module_state(&evaluation, 3), &expected, "{completed:?}");
    }
}

#[test]
fn t3_a_module_without_requirements_satisfies_a_prerequisite_once_it_is_unlocked() {
    let reading = module(1, vec![plain(10)]);
    let graded = requiring(module(2, vec![required(20)]), &[1]);
    let open = evaluate(
        &course(vec![reading.clone(), graded.clone()]),
        &done(&[]),
        ten(),
    );
    assert_eq!(module_state(&open, 1), &State::Available);
    assert_eq!(module_state(&open, 2), &State::Available);
    assert!(!open.module(id(1)).unwrap().complete, "never complete (Q4)");

    let mut later = reading;
    later.unlock_at = Some(ten() + Duration::from_secs(1));
    let waiting = evaluate(&course(vec![later, graded]), &done(&[]), ten());
    assert!(module_state(&waiting, 1).is_locked());
    assert_eq!(
        module_state(&waiting, 2),
        &locked(vec![prerequisites(&[1])])
    );
}

#[test]
fn t3_a_module_without_requirements_passes_its_own_lock_down_a_chain() {
    // C requires B (no requirements) requires A (one requirement).
    let rules = course(vec![
        module(1, vec![required(10)]),
        requiring(module(2, vec![plain(20)]), &[1]),
        requiring(module(3, vec![required(30)]), &[2]),
    ]);
    let before = evaluate(&rules, &done(&[]), ten());
    assert_eq!(module_state(&before, 2), &locked(vec![prerequisites(&[1])]));
    assert_eq!(module_state(&before, 3), &locked(vec![prerequisites(&[2])]));
    let after = evaluate(&rules, &done(&[10]), ten());
    assert_eq!(module_state(&after, 2), &State::Available);
    assert_eq!(module_state(&after, 3), &State::Available);
}

#[test]
fn t4_a_sequential_module_opens_items_in_order() {
    let items = vec![required(10), plain(11), required(12)];
    let mut sequential = module(1, items.clone());
    sequential.require_sequential = true;
    let wait_for_first = locked(vec![LockReason::Sequential { item_id: id(10) }]);
    let cases: [(&[u128], [State; 3]); 3] = [
        (
            &[],
            [
                State::Available,
                wait_for_first.clone(),
                wait_for_first.clone(),
            ],
        ),
        (
            &[10],
            [State::Available, State::Available, State::Available],
        ),
        // The last requirement done alone opens nothing before it.
        (
            &[12],
            [
                State::Available,
                wait_for_first.clone(),
                wait_for_first.clone(),
            ],
        ),
    ];
    let rules = course(vec![sequential]);
    for (completed, expected) in cases {
        let evaluation = evaluate(&rules, &done(completed), ten());
        let states: Vec<&State> = [10, 11, 12]
            .iter()
            .map(|&n| item_state(&evaluation, n))
            .collect();
        assert_eq!(states, expected.iter().collect::<Vec<_>>(), "{completed:?}");
    }
    let free = evaluate(&course(vec![module(1, items)]), &done(&[]), ten());
    for n in [10, 11, 12] {
        assert_eq!(item_state(&free, n), &State::Available, "{n}");
    }
}

#[test]
fn t4_a_sequential_reason_names_the_first_incomplete_requirement() {
    let mut sequential = module(1, vec![required(10), required(11), plain(12)]);
    sequential.require_sequential = true;
    let evaluation = evaluate(&course(vec![sequential]), &done(&[10]), ten());
    assert_eq!(item_state(&evaluation, 11), &State::Available);
    assert_eq!(
        item_state(&evaluation, 12),
        &locked(vec![LockReason::Sequential { item_id: id(11) }])
    );
}

#[test]
fn t5_unpublished_content_is_absent_and_blocks_nothing() {
    // `load_course_rules` leaves unpublished modules and items out; a prerequisite id that names
    // no module of the rules is an unpublished one, and is met.
    let rules = course(vec![requiring(module(2, vec![required(20)]), &[1])]);
    let evaluation = evaluate(&rules, &done(&[]), ten());
    assert_eq!(module_state(&evaluation, 2), &State::Available);
    assert_eq!(evaluation.module(id(1)), None);
}

#[test]
fn t6_completion_rules_and_course_completion() {
    let mut one = module(2, vec![required(20), required(21)]);
    one.completion_rule = CompletionRule::One;
    let rules = course(vec![
        module(1, vec![required(10), required(11)]),
        one,
        module(3, vec![plain(30)]),
    ]);
    let cases: [(&[u128], bool, bool, bool); 5] = [
        (&[], false, false, false),
        (&[10], false, false, false),
        (&[10, 11], true, false, false),
        (&[21], false, true, false),
        (&[10, 11, 20], true, true, true),
    ];
    for (completed, all_done, one_done, course_done) in cases {
        let evaluation = evaluate(&rules, &done(completed), ten());
        assert_eq!(evaluation.module(id(1)).unwrap().complete, all_done);
        assert_eq!(evaluation.module(id(2)).unwrap().complete, one_done);
        assert!(
            !evaluation.module(id(3)).unwrap().complete,
            "no requirement"
        );
        assert_eq!(evaluation.course_complete(), course_done, "{completed:?}");
    }
}

#[test]
fn t6_a_course_without_requirements_never_completes() {
    let rules = course(vec![module(1, vec![plain(10)]), module(2, vec![])]);
    assert!(!evaluate(&rules, &done(&[10]), ten()).course_complete());
    assert!(!evaluate(&CourseRules::default(), &done(&[]), ten()).course_complete());
}

#[test]
fn t7_a_locked_modules_items_are_locked_by_it_alone() {
    let mut gated = requiring(module(2, vec![required(20), plain(21), required(22)]), &[1]);
    gated.require_sequential = true;
    let rules = course(vec![module(1, vec![required(10)]), gated]);
    // Item 20 done and item 22 done: the module's lock still decides.
    let evaluation = evaluate(&rules, &done(&[20, 22]), ten());
    assert!(module_state(&evaluation, 2).is_locked());
    for n in [20, 21, 22] {
        assert_eq!(
            item_state(&evaluation, n),
            &locked(vec![LockReason::ModuleLocked]),
            "{n}"
        );
    }
    assert!(evaluation.item(id(20)).unwrap().complete);
}

#[test]
fn b4_a_completed_item_stays_complete_whatever_its_requirement_now_is() {
    for requirement in [
        Requirement::View,
        Requirement::MarkDone,
        Requirement::Watch { min_percent: 100 },
    ] {
        let item = ItemRules {
            id: id(10),
            requirement: Some(requirement),
        };
        let evaluation = evaluate(&course(vec![module(1, vec![item])]), &done(&[10]), ten());
        assert!(
            evaluation.module(id(1)).unwrap().complete,
            "{requirement:?}"
        );
        assert!(evaluation.item(id(10)).unwrap().complete, "{requirement:?}");
    }
}

#[test]
fn a_cycle_that_got_into_the_data_keeps_its_modules_locked() {
    let rules = course(vec![
        requiring(module(1, vec![plain(10)]), &[2]),
        requiring(module(2, vec![plain(20)]), &[1]),
        requiring(module(3, vec![plain(30)]), &[3]),
        module(4, vec![plain(40)]),
    ]);
    let evaluation = evaluate(&rules, &done(&[]), ten());
    assert_eq!(
        module_state(&evaluation, 1),
        &locked(vec![prerequisites(&[2])])
    );
    assert_eq!(
        module_state(&evaluation, 2),
        &locked(vec![prerequisites(&[1])])
    );
    assert_eq!(
        module_state(&evaluation, 3),
        &locked(vec![prerequisites(&[3])])
    );
    assert_eq!(module_state(&evaluation, 4), &State::Available);
}

#[test]
fn a_prerequisite_named_twice_is_listed_once() {
    let rules = course(vec![
        module(1, vec![required(10)]),
        requiring(module(2, vec![]), &[1, 1]),
    ]);
    let evaluation = evaluate(&rules, &done(&[]), ten());
    assert_eq!(
        module_state(&evaluation, 2),
        &locked(vec![prerequisites(&[1])])
    );
}

/// A generated module before ids are given: its unlock offset in seconds from now, the indices
/// of the modules it requires (any module, itself included, so cycles are generated too), its
/// switches, and whether each item has a requirement.
type RawModule = (Option<i16>, Vec<usize>, bool, bool, Vec<bool>);

fn raw_course() -> impl Strategy<Value = (Vec<RawModule>, Vec<bool>)> {
    let raw_module = (
        proptest::option::of(-100i16..100),
        proptest::collection::vec(0usize..8, 0..4),
        any::<bool>(),
        any::<bool>(),
        proptest::collection::vec(any::<bool>(), 0..5),
    );
    (
        proptest::collection::vec(raw_module, 0..8),
        proptest::collection::vec(any::<bool>(), 40),
    )
}

/// The course, and the completed set, built from the generated values. Module `m` has id `m`
/// and its item `i` has id `100 * (m + 1) + i`.
fn build(raw: &[RawModule], done_bits: &[bool]) -> (CourseRules, Vec<Uuid>) {
    let mut completed = Vec::new();
    let mut bit = 0;
    let modules = raw
        .iter()
        .enumerate()
        .map(|(m, (offset, requires, sequential, one, items))| {
            let m = m as u128;
            let items = items
                .iter()
                .enumerate()
                .map(|(i, has)| {
                    let item = id(100 * (m + 1) + i as u128);
                    if done_bits[bit % done_bits.len()] {
                        completed.push(item);
                    }
                    bit += 1;
                    ItemRules {
                        id: item,
                        requirement: has.then_some(Requirement::MarkDone),
                    }
                })
                .collect();
            ModuleRules {
                id: id(m),
                unlock_at: offset.map(|s| {
                    let shift = Duration::from_secs(s.unsigned_abs().into());
                    if s < 0 { ten() - shift } else { ten() + shift }
                }),
                prerequisite_ids: requires.iter().map(|&r| id(r as u128)).collect(),
                require_sequential: *sequential,
                completion_rule: if *one {
                    CompletionRule::One
                } else {
                    CompletionRule::All
                },
                items,
            }
        })
        .collect();
    (CourseRules { modules }, completed)
}

/// Every reason names only what the learner can see in the same course (B14), and a locked
/// state always has a reason.
fn check_reasons(rules: &CourseRules, evaluation: &Evaluation) {
    let module_ids: HashSet<Uuid> = rules.modules.iter().map(|m| m.id).collect();
    for module in &rules.modules {
        let state = &evaluation.module(module.id).expect("every module").state;
        assert!(!matches!(state, State::Locked(r) if r.is_empty()));
        for reason in state.reasons() {
            match reason {
                LockReason::UnlockAt { at } => assert!(*at > ten()),
                LockReason::Prerequisites { module_ids: named } => {
                    assert!(!named.is_empty());
                    assert!(named.iter().all(|n| module_ids.contains(n)));
                }
                other => panic!("a module reason {other:?}"),
            }
        }
        for (position, item) in module.items.iter().enumerate() {
            let item_state = &evaluation.item(item.id).expect("every item").state;
            if let [LockReason::Sequential { item_id }] = item_state.reasons() {
                let earlier = &module.items[..position];
                assert!(
                    earlier
                        .iter()
                        .any(|e| e.id == *item_id && e.requirement.is_some())
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// T8: the same inputs give the same output, whatever order the completed set was built
    /// in, and every item whose module is locked is locked by it alone.
    #[test]
    fn t8_evaluate_is_deterministic_and_a_locked_module_locks_its_items(
        (raw, done_bits) in raw_course()
    ) {
        let (rules, completed) = build(&raw, &done_bits);
        let forward: HashSet<Uuid> = completed.iter().copied().collect();
        let backward: HashSet<Uuid> = completed.iter().rev().copied().collect();
        let first = evaluate(&rules, &forward, ten());
        let second = evaluate(&rules.clone(), &backward, ten());
        prop_assert_eq!(&first, &second);
        for module in &rules.modules {
            let state = &first.module(module.id).expect("every module").state;
            for item in &module.items {
                let item_state = &first.item(item.id).expect("every item").state;
                if state.is_locked() {
                    prop_assert_eq!(item_state, &State::Locked(vec![LockReason::ModuleLocked]));
                }
            }
        }
        check_reasons(&rules, &first);
    }
}
