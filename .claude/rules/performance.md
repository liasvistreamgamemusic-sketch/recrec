# Performance Optimization

## Model Selection Strategy

Current lineup (verified 2026-07-03): Claude 5 family (Fable 5, Sonnet 5), Opus 4.8, Haiku 4.5.

**Haiku 4.5** (`claude-haiku-4-5-20251001`) — fastest/cheapest:
- High-frequency worker agents in multi-agent systems
- Mechanical subtasks: classification, extraction, formatting, simple lookups

**Sonnet 5** (`claude-sonnet-5`) — default workhorse:
- Main development work and day-to-day coding
- Orchestrating multi-agent workflows

**Opus 4.8** (`claude-opus-4-8`) — deep reasoning:
- Complex debugging, architectural decisions
- Fast mode (`/fast`) gives the same Opus quality with faster output

**Fable 5** (`claude-fable-5`) — Mythos-class tier above Opus; most capable
generally available model:
- Hardest problems: architecture, high-stakes review, thorny debugging,
  research and analysis
- Framework/rules authoring; tasks where judgment quality dominates cost

Routing rule: default to Sonnet 5; drop to Haiku 4.5 for mechanical volume;
escalate to Opus 4.8 / Fable 5 when reasoning depth, ambiguity, or blast
radius is high.

When building AI applications, default to the latest models above. Model IDs
cached in plugin skills and old docs go stale — trust the harness environment
context and official Anthropic docs, never memory or copied examples.

## Context Window Management

Avoid last 20% of context window for:
- Large-scale refactoring
- Feature implementation spanning multiple files
- Debugging complex interactions

Lower context sensitivity tasks:
- Single-file edits
- Independent utility creation
- Documentation updates
- Simple bug fixes
