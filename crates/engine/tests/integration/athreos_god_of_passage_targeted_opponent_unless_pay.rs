//! Production-path coverage for Athreos, God of Passage — the declared-target
//! unless-payer class ("[effect] unless target opponent/target player pays
//! [cost]").
//!
//! Oracle (trigger line under test):
//!   "Whenever another creature you control dies, return it to its owner's hand
//!    unless target opponent pays 3 life."
//!
//! Unlike the anaphoric punisher class ("... unless they/that opponent pays"),
//! the payer here is DECLARED as a target INSIDE the unless clause and chosen at
//! stack placement (CR 603.3d). The trigger must therefore surface a player
//! target slot bound to the controller's OPPONENTS, and the resulting
//! `UnlessPayment` prompt must go to the CHOSEN opponent — never the controller.
//!
//! CR ANCHORS (verified against docs/MagicCompRules.txt):
//!   * CR 115.1   — targets are declared as the spell/ability goes on the stack.
//!   * CR 118.12a — "[Do something] unless [a player does something else]."
//!   * CR 119.4   — paying life loses that much life.
//!   * CR 603.3d  — a triggered ability with no legal target is removed from the stack.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::game::triggers::process_triggers;
use engine::types::ability::{ContinuousModification, Duration, TargetFilter, TargetRef};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::{Keyword, ProtectionTarget};
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const P2: PlayerId = PlayerId(2);

const ATHREOS_TRIGGER: &str = "Whenever another creature you control dies, \
     return it to its owner's hand unless target opponent pays 3 life.";

/// True when `player`'s `zone` holds an object named `name`. Name-based (not
/// ObjectId-based) so it survives the CR 400.7 new-object-per-zone-change churn
/// a dying / returning creature goes through.
fn name_in_zone(
    runner: &engine::game::scenario::GameRunner,
    player: PlayerId,
    zone: Zone,
    name: &str,
) -> bool {
    let state = runner.state();
    let p = state
        .players
        .iter()
        .find(|p| p.id == player)
        .expect("player exists");
    let ids = match zone {
        Zone::Hand => &p.hand,
        Zone::Graveyard => &p.graveyard,
        _ => panic!("name_in_zone only supports Hand/Graveyard"),
    };
    ids.iter()
        .any(|id| state.objects.get(id).is_some_and(|o| o.name == name))
}

/// Mark `victim` with lethal damage, run state-based actions so it dies, then
/// process the triggers it produced. Leaves the runner with the Athreos trigger
/// pending (awaiting target selection).
fn kill_and_trigger(runner: &mut engine::game::scenario::GameRunner, victim: ObjectId) {
    runner
        .state_mut()
        .objects
        .get_mut(&victim)
        .unwrap()
        .damage_marked = 99;

    let mut events = Vec::new();
    engine::game::sba::check_state_based_actions(runner.state_mut(), &mut events);
    process_triggers(runner.state_mut(), &events);
}

/// Drive priority/resolution until the Athreos trigger surfaces its declared
/// player-target slot; choose `chosen` (must be one of the controller's
/// opponents). Panics if the trigger never asks for a target.
fn select_trigger_target(runner: &mut engine::game::scenario::GameRunner, chosen: PlayerId) {
    for _ in 0..64 {
        match runner.state().waiting_for.clone() {
            WaitingFor::TriggerTargetSelection { target_slots, .. } => {
                assert!(
                    target_slots[0]
                        .legal_targets
                        .contains(&TargetRef::Player(chosen)),
                    "chosen opponent must be a legal target, slots = {target_slots:?}"
                );
                assert!(
                    !target_slots[0]
                        .legal_targets
                        .contains(&TargetRef::Player(P0)),
                    "the controller (P0) must NOT be a legal opponent target"
                );
                runner
                    .act(GameAction::SelectTargets {
                        targets: vec![TargetRef::Player(chosen)],
                    })
                    .expect("targeting the opponent must succeed");
                return;
            }
            WaitingFor::Priority { .. } => {
                if runner.act(GameAction::PassPriority).is_err() {
                    break;
                }
            }
            other => panic!("unexpected waiting state before target selection: {other:?}"),
        }
    }
    panic!("Athreos trigger never surfaced a target-player selection");
}

/// Advance to the `UnlessPayment` prompt (resolving the trigger off the stack),
/// asserting the payer is `expected`. Returns once the prompt is reached.
fn advance_to_unless_payment(runner: &mut engine::game::scenario::GameRunner, expected: PlayerId) {
    for _ in 0..64 {
        match runner.state().waiting_for.clone() {
            WaitingFor::UnlessPayment { player, .. } => {
                assert_eq!(
                    player, expected,
                    "the unless-payer must be the chosen opponent, not the controller"
                );
                return;
            }
            WaitingFor::Priority { .. } => {
                if runner.act(GameAction::PassPriority).is_err() {
                    break;
                }
            }
            other => panic!("unexpected waiting state before UnlessPayment: {other:?}"),
        }
    }
    panic!("the Athreos trigger never produced an UnlessPayment prompt");
}

/// Build a 3-player game with Athreos under P0 and a vanilla creature P0
/// controls. Returns `(runner, victim_id)`.
fn setup_three_player() -> (engine::game::scenario::GameRunner, ObjectId) {
    let mut scenario = GameScenario::new_n_player(3, 42);
    scenario.at_phase(engine::types::phase::Phase::PreCombatMain);

    scenario.add_creature_from_oracle(P0, "Athreos, God of Passage", 0, 0, ATHREOS_TRIGGER);
    let victim = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();

    let runner = scenario.build();
    (runner, victim)
}

/// CR 115.1 + CR 118.12a + CR 119.4 (V6 decline): the chosen opponent (P2)
/// receives the prompt; declining lets the effect happen — the dead creature
/// returns to its owner (P0)'s hand and P2's life is unchanged.
#[test]
fn athreos_targets_chosen_opponent_decline_returns_creature() {
    let (mut runner, victim) = setup_three_player();
    let p2_life_before = runner.life(P2);

    kill_and_trigger(&mut runner, victim);
    select_trigger_target(&mut runner, P2);
    advance_to_unless_payment(&mut runner, P2);

    runner
        .act(GameAction::PayUnlessCost { pay: false })
        .expect("declining the unless-cost must be accepted");
    runner.advance_until_stack_empty();

    // CR 118.12a: declining means the effect happens — the creature returns to
    // its owner's hand.
    assert!(
        name_in_zone(&runner, P0, Zone::Hand, "Grizzly Bears"),
        "declining must return the dead creature to its owner (P0)'s hand"
    );
    assert!(
        !name_in_zone(&runner, P0, Zone::Graveyard, "Grizzly Bears"),
        "the creature must have left the graveyard for hand"
    );
    // CR 119.4: declining costs no life.
    assert_eq!(
        runner.life(P2),
        p2_life_before,
        "declining must not change the chosen opponent's life"
    );
}

/// CR 118.12a + CR 119.4 (V7 pay): when the chosen opponent (P2) pays 3 life,
/// the effect is suppressed — the creature stays in the graveyard and P2 loses
/// exactly 3 life.
#[test]
fn athreos_chosen_opponent_pays_keeps_creature_in_graveyard() {
    let (mut runner, victim) = setup_three_player();
    let p2_life_before = runner.life(P2);

    kill_and_trigger(&mut runner, victim);
    select_trigger_target(&mut runner, P2);
    advance_to_unless_payment(&mut runner, P2);

    runner
        .act(GameAction::PayUnlessCost { pay: true })
        .expect("paying the 3-life unless-cost must be accepted");
    runner.advance_until_stack_empty();

    // CR 118.12a: paying suppresses the effect — the creature stays dead.
    assert!(
        name_in_zone(&runner, P0, Zone::Graveyard, "Grizzly Bears"),
        "paying the unless-cost must keep the creature in the graveyard"
    );
    assert!(
        !name_in_zone(&runner, P0, Zone::Hand, "Grizzly Bears"),
        "paying must NOT return the creature to hand"
    );
    // CR 119.4: paying 3 life loses exactly 3 life.
    assert_eq!(
        runner.life(P2),
        p2_life_before - 3,
        "paying the unless-cost must deduct exactly 3 life from the chosen opponent"
    );
}

/// CR 603.3d (R1 — required-target removal): Athreos is a MANDATORY trigger.
/// When NO opponent can be legally targeted (here: the sole reachable opponents
/// all have protection from everything, so the declared opponent target has no
/// legal choice), the trigger is REMOVED from the stack and the creature is NOT
/// returned. This proves the opponent is a real required target (CR 115.1), not
/// a silent no-op — reverting the target-slot wiring makes this test fail (the
/// trigger would resolve with no payer and wrongly return the creature, or
/// never prompt at all).
#[test]
fn athreos_trigger_removed_when_no_legal_opponent_target() {
    let (mut runner, victim) = setup_three_player();

    // CR 702.16j: grant every opponent protection from everything, so the
    // declared "target opponent" slot has no legal target. Driven through the
    // single TCE authority — a real continuous-effect grant, not a test hook.
    for opponent in [P1, P2] {
        runner.state_mut().add_transient_continuous_effect(
            ObjectId(0),
            opponent,
            Duration::Permanent,
            TargetFilter::SpecificPlayer { id: opponent },
            vec![ContinuousModification::AddKeyword {
                keyword: Keyword::Protection(ProtectionTarget::Everything),
            }],
            None,
        );
    }

    kill_and_trigger(&mut runner, victim);
    runner.advance_until_stack_empty();

    // CR 603.3d: with no legal opponent target, the trigger is removed — no
    // UnlessPayment prompt ever appears.
    assert!(
        !matches!(runner.state().waiting_for, WaitingFor::UnlessPayment { .. }),
        "a mandatory trigger with no legal target must NOT reach an UnlessPayment prompt"
    );
    assert!(
        !matches!(
            runner.state().waiting_for,
            WaitingFor::TriggerTargetSelection { .. }
        ),
        "a trigger with no legal target must not hang on target selection"
    );
    // The creature stays dead — the removed trigger never returned it.
    assert!(
        name_in_zone(&runner, P0, Zone::Graveyard, "Grizzly Bears"),
        "the removed trigger must not return the creature to hand (CR 603.3d)"
    );
    assert!(
        !name_in_zone(&runner, P0, Zone::Hand, "Grizzly Bears"),
        "the creature must remain in the graveyard, not return to hand"
    );
}
