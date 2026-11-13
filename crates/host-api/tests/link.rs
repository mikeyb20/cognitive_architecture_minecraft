// The runner ↔ bot wire format (spec §22), byte for byte.

use host_api::link::{BotConfig, LinkReply, LinkRequest, RecipeHint, Thresholds};
use host_api::reflex::ReflexThresholds;
use host_api::{Inventory, Outcome};
use serde_json::json;

#[test]
fn requests_and_replies_on_the_wire() {
    let say = LinkRequest::Say { text: "hi".into() };
    assert_eq!(
        serde_json::to_value(&say).unwrap(),
        json!({"op": "say", "text": "hi"})
    );
    let perceive = serde_json::to_value(&LinkRequest::Perceive).unwrap();
    assert_eq!(perceive, json!({"op": "perceive"}));
    let hello = serde_json::to_value(LinkRequest::Hello {
        config: BotConfig::default(),
    })
    .unwrap();
    assert_eq!(hello["op"], "hello");
    assert_eq!(
        hello["config"]["skills"],
        json!({"searchRadius": 64, "oreDepth": 16, "followRange": 3,
               "followMs": 5000, "restMs": 5000, "searchStep": 24})
    );
    assert_eq!(
        hello["config"]["limits"],
        json!({"maxVisibleBlocks": 32, "maxPerKind": 4})
    );
    assert_eq!(
        hello["config"]["thresholds"]["criticalHealth"],
        json!(0.2f32)
    );
    assert_eq!(
        hello["config"]["thresholds"]["starvingFood"],
        json!(0.05f32)
    );
    let ready: LinkReply =
        serde_json::from_value(json!({"reply": "ready", "names": ["stone"]})).unwrap();
    assert_eq!(
        ready,
        LinkReply::Ready {
            names: vec!["stone".into()]
        }
    );
    let failed: LinkReply =
        serde_json::from_value(json!({"reply": "failed", "message": "no"})).unwrap();
    assert_eq!(
        failed,
        LinkReply::Failed {
            message: "no".into()
        }
    );
    let done = LinkReply::Done {
        outcome: Outcome {
            success: true,
            progress: 1.0,
            items_gained: Inventory::default(),
            damage: 0.0,
            new_things: vec![],
            blocked: false,
            duration_ticks: 40,
        },
    };
    let back: LinkReply = serde_json::from_str(&serde_json::to_string(&done).unwrap()).unwrap();
    assert_eq!(back, done);
}

#[test]
fn recipe_hints_come_from_the_tech_tree() {
    assert_eq!(
        RecipeHint::for_item("iron_ingot"),
        Some(RecipeHint {
            station: Some("furnace".into()),
            inputs: vec![("raw_iron".into(), 1)],
        })
    );
    assert_eq!(
        RecipeHint::for_item("oak_planks"),
        Some(RecipeHint {
            station: None,
            inputs: vec![("oak_log".into(), 1)],
        })
    );
    assert_eq!(RecipeHint::for_item("diamond"), None);
}

#[test]
fn thresholds_copy_the_reflex_layer() {
    let t: Thresholds = ReflexThresholds {
        critical_health: 0.3,
        starving_food: 0.1,
    }
    .into();
    assert_eq!((t.critical_health, t.starving_food), (0.3, 0.1));
}
