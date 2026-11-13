//! Recipes kept in procedural memory (01 §4.11, §5.2, §5.3): the longest
//! shown recipe is kept, recalled only while its item is alive, and each
//! recall or followed step is an access.

use super::*;
use crate::control::teacher::{Lesson, Recipe};

fn step(skill: Skill, thing: &str) -> PlanStep {
    PlanStep {
        skill,
        thing: thing.into(),
        task: TaskType::Building,
    }
}

fn recipe(steps: &[(Skill, &str)]) -> Recipe {
    Recipe {
        goal: "shelter".into(),
        steps: steps.iter().map(|(s, t)| step(*s, t)).collect(),
    }
}

fn accesses(b: &Brain) -> u32 {
    let id = b.mem.get("recipe:shelter").expect("stored");
    b.mem.entry(id).item.access_count
}

/// A brain that was shown `r` for "shelter" at tick 0.
fn shown(r: &Recipe) -> Brain {
    let mut b = brain();
    b.remember_taught("recipe:shelter", crate::memory::Tier::Procedural, 0);
    b.keep_recipe("shelter", r);
    b
}

#[test]
fn the_longest_shown_recipe_is_kept() {
    let a = recipe(&[(Skill::Craft, "item:oak_planks"), (Skill::Shelter, "")]);
    let b2 = recipe(&[(Skill::Chop, "block:oak_log"), (Skill::Shelter, "")]);
    let long = recipe(&[
        (Skill::Chop, "block:oak_log"),
        (Skill::Craft, "item:oak_planks"),
        (Skill::Shelter, ""),
    ]);
    let mut b = shown(&a);
    b.keep_recipe("shelter", &b2);
    assert_eq!(b.recipes["shelter"], a, "same length: the first stays");
    b.keep_recipe("shelter", &long);
    assert_eq!(b.recipes["shelter"], long, "longer replaces");
    b.keep_recipe("shelter", &recipe(&[(Skill::Shelter, "")]));
    assert_eq!(b.recipes["shelter"], long, "shorter does not");
}

#[test]
fn a_recipe_is_recalled_only_while_its_item_lives_and_recall_is_an_access() {
    let r = recipe(&[(Skill::Shelter, "")]);
    let mut b = brain();
    assert_eq!(b.recall_recipe("shelter", 5), None, "never shown");
    // Steps without a memory item (e.g. a learned crafting recipe key with
    // no taught steps) are not recalled either.
    b.recipes.insert("shelter".into(), r.clone());
    assert_eq!(b.recall_recipe("shelter", 5), None, "no item");
    let mut b = shown(&r);
    let before = accesses(&b);
    assert_eq!(b.recall_recipe("shelter", 5), Some(r.clone()));
    assert_eq!(accesses(&b), before + 1);
    let id = b.mem.get("recipe:shelter").unwrap();
    assert_eq!(b.mem.entry(id).item.t_last_access, 5);
    b.mem.prune(id);
    assert_eq!(b.recall_recipe("shelter", 6), None, "forgotten");
    assert_eq!(b.recall_recipe("stick", 6), None);
}

#[test]
fn following_a_step_is_an_access_of_the_recipe() {
    let r = recipe(&[(Skill::Chop, "block:oak_log"), (Skill::Shelter, "")]);
    let mut b = shown(&r);
    b.lesson = Some(Lesson::new(r));
    let act = |skill, target: &str, success, tick| Pending {
        tick,
        skill: Some(skill),
        target: Some(target.into()),
        ..external(Outcome {
            success,
            ..Outcome::default()
        })
    };
    let before = accesses(&b);
    let out = |success| Outcome {
        success,
        ..Outcome::default()
    };
    b.learn_lesson(&act(Skill::Chop, "block:oak_log", false, 3), &out(false));
    b.learn_lesson(&act(Skill::Explore, "region:1", true, 4), &out(true));
    assert_eq!(accesses(&b), before, "no step taken");
    b.learn_lesson(&act(Skill::Chop, "block:oak_log", true, 7), &out(true));
    assert_eq!(accesses(&b), before + 1);
    let id = b.mem.get("recipe:shelter").unwrap();
    assert_eq!(b.mem.entry(id).item.t_last_access, 7);
    assert_eq!(b.lesson.as_ref().map(|l| l.next), Some(1));
}
