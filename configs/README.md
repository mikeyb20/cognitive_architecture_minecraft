# Configs

- `archetypes/`: explorer, builder, guardian, helper (#44). Parameter means from spec §19, drawn per agent from Normal(mean, 10%). Proposals until the sim pilot.
- `scenarios/`: first night, commanded tasks, open day, harsh episode, lava bridge, long horizon, incubation, far food, dusk errand, hostile at the food, expedition, held-out terrain (spec §21, §20.1, §27 step 15).
- `experiments/`: arms × scenarios × seeds × LLM tier (spec §24, §26). `floor` (default true) turns on the shared survival floor of plan §5.0: the player asks for a shelter at each dusk, for every arm.
