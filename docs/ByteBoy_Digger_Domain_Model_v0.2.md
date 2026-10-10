# ByteBoy Digger Domain Model v0.2

> **Status:** Domain Model Consolidation  
> **Product:** ByteBoy Digger — AI Creator  
> **Methodology:** Project-centric  
> **Scope:** 产品领域模型；暂不涉及 UI、数据库 Schema、具体 Agent Framework 或技术实现。

---

## 0. Executive Summary

ByteBoy Digger 是面向文字、图片、音视频与内容网站的 **AI Creator**。首期覆盖三个 Creator Domain：

1. **Social** — 自媒体与持续内容运营
2. **Writing** — 长篇叙事与研究型写作
3. **Site** — AGC/OGC 内容网站

Digger 建立在更上层的 **Project-centric** 哲学之上：

> **不要只优化 Output，要优化产生 Output 的 Project。**  
> Output 会消耗，Project 会积累。

Project 持续积累：

```text
Knowledge
+ Rules
+ Structure
+ Decisions
+ Skills
+ Workflows
+ Assets
+ Feedback
+ Production History
```

经过 Social、Writing、Site 三轮 End-to-End 压力测试，领域边界收敛为一句话：

> **Core owns transitions; Domain owns meaning.**

Digger Core 不尝试定义一个万能的 Content / Document / Artifact。它负责 Human 与 Agent 如何理解 Project、采取行动、治理变化、形成正式状态、与外部世界交互，并保留 Project 演化的因果历史；Social、Writing、Site 则分别拥有自己的专业语义。

---

# 1. 产品哲学与边界

## 1.1 Project-centric 是哲学，不是万能产品模型

```text
Project-centric Philosophy
        ↓
Project Methodology
        ↓
Domain Products
   ├── ByteBoy Dozer — AI Coder
   └── ByteBoy Digger — AI Creator
```

- **Dozer**：以代码为主要生产对象的软件开发领域产品。
- **Digger**：以文字、图片、音视频和内容网站为主要生产对象的 Creator 产品。
- Dozer 可以被用于生产 Digger，但这不意味着 Dozer 包含 Digger 的全部领域语义。

产品必须保留明确领域边界：

> 到处是路，反而也就没有路了。

因此 Digger 不应演变成 generic “everything workspace”。

## 1.2 Project 不是 Output

```text
Project
≠ Content
≠ Document
≠ Work
≠ Brand
≠ Site
≠ Account
```

工作定义：

> **Project 是一组 Human、Agent 与 Tool 围绕某个长期 Creator 目标持续工作的边界，并承载其资源、知识、规则、历史和生产活动。**

Project 更接近：

```text
Boundary
+
Runtime
+
Persistent Context
```

而不是万能内容对象。

---

# 2. 三个 Creator Domain

## 2.1 Social

```text
Social Project
│
├── Brand / Creator
├── Audience
├── Channel
├── Strategy
├── Topic / Content Pillar
├── Series / Campaign
├── Calendar
├── Content
├── Variant
├── Media Production
├── Publication
├── Analytics
├── Attribution
└── Feedback
```

时间形态：

```text
Produce → Publish → Learn → Produce
```

Domain Root：**Brand / Creator**

关键关系：

```text
Content
  1
  ├──── N Variant
  └──── N Publication

Publication
  N ──── 1 Channel
```

核心规则：

> **Content ≠ Publication**

一个 Content 可以形成多个平台 Variant 和多个外部 Publication。

## 2.2 Writing

```text
Writing Project
│
├── Work
├── Intent / Thesis
├── Structure
│   ├── Outline
│   ├── Part
│   ├── Chapter
│   ├── Section
│   └── Scene
├── Knowledge / Research
├── Work Truth
│   ├── Narrative Canon
│   ├── Terminology
│   ├── Thesis
│   ├── Author Decision
│   └── Accepted Claim
├── Manuscript
├── Revision
├── Edition
└── Export / Distribution
```

时间形态：

```text
Research → Write → Revise → Complete / Edition
```

Domain Root：**Work**

重要语义：

```text
Structure  = 写在哪里
Truth      = 什么能写 / 必须服从什么
Knowledge  = 拿什么写
Manuscript = 已经写出来什么
```

Narrative 与 Research Writing 的核心约束不同：

```text
Narrative core constraint = Canon
Research core constraint  = Evidence
```

但二者都可以通过 Core 的 Authority / Change Governance 机制管理。

## 2.3 Site

```text
Site Project
│
├── Site
├── Information Architecture
│   ├── Route
│   ├── Navigation
│   └── Taxonomy
├── Content Model
│   ├── Schema
│   ├── Collection
│   ├── Field
│   └── Relation
├── Content Program
├── Content
│   ├── Entry
│   ├── Page
│   └── Block
├── Presentation
│   ├── Design Token
│   ├── Component
│   ├── Template
│   └── Layout
├── Deployment
├── SEO
└── Analytics
```

时间形态：

```text
Model → Produce → Publish → Measure → Maintain
```

Domain Root：**Site**

AGC/OGC Site 的核心不是简单 Content Collection，而是 **Content Program**：

```text
Business
+ Audience
+ Topic Space
+ Keyword Space
+ Schema
+ Template
+ Rules
        ↓
Page 1 ... Page 10,000
```

---

# 3. Core / Domain 边界原则

三轮压力测试得到稳定边界：

> **Core owns transitions; Domain owns meaning.**

Domain 回答：

```text
What is Brand?
What is Chapter?
What is Claim?
What is Publication?
What is Schema?
What is Page?
What is Edition?
```

Core 回答：

```text
Who acted?
Why?
What context was used?
What changed?
From what to what?
Was it allowed?
What supported the decision?
Was review required?
When did it become official?
What happened externally?
Why does the current state exist?
```

进一步原则：

> **统一方法论，不统一领域语义。**

---

# 4. Digger Core v0.2

```text
Digger Core
│
├── Project State
├── Actor
├── Knowledge & Intent
├── Production & Change
├── External Interaction
│
└── Cross-cutting
    ├── Context Assembly
    ├── Provenance
    ├── Impact Analysis
    └── Project History / Causality
```

---

# 5. Project State

## 5.1 Project

```text
Project
├── Identity
├── Metadata
├── Domain
├── Configuration
└── Lifecycle
```

Core 生命周期保持极薄：

```text
Active
Paused
Archived
```

业务生命周期留给 Domain：

```text
Writing:
Drafting → Editing → Edition

Social:
continuous operation

Site:
Development → Live → Maintenance
```

Digger Core 不定义统一 Creator Workflow。

## 5.2 Authority / Project Truth

产品层可以继续使用 **Project Truth**，Core 中更精确的概念是 **Authority**。

Project Truth 不是一个巨大的 Truth Store，而是：

> 某些 Domain State / Knowledge / Decision 被赋予权威状态，从而约束未来生产。

```text
Knowledge / Domain State
       │
       ├── ordinary
       ├── proposed
       ├── accepted
       ├── decision
       └── authoritative
                ↑
          Project Truth
```

可能表现为：

```text
Fact
Canon
Decision
Rule
Contract
Accepted Claim
Strategy
Preference
```

但这些不必成为统一 Core 类型。

重要修正：

> **Authority 不复制 Domain Truth；Authority 是施加在 Domain State 上的治理语义。**

例如：

```text
Brand.voice = "technical"
        ↑
 authoritative within Brand scope
```

Authority 需要 Scope：

```text
Authority
├── Target / Value
├── Scope
├── Priority
├── Provenance
├── Rationale
├── Status
└── Validity
```

## 5.3 Snapshot

> **Snapshot = 对某一时刻 Project State 或某个 Scope State 的不可变引用。**

```text
Snapshot
├── Scope
├── State Reference
├── Created At
├── Provenance
└── Label
```

三个领域均需要：

```text
Social  → Content / Variant Snapshot → Publication
Writing → Manuscript Snapshot → Edition
Site    → Site Snapshot → Deployment
```

区别：

```text
Commit
= S₀ → S₁ 的变化事件

Snapshot
= 对 Sₙ 的稳定引用
```

---

# 6. Actor Model

## 6.1 Actor

Human 和 Agent 是 Actor；Tool 不是 Actor。

```text
Project
├── Actor
│   ├── Human
│   └── Agent
└── Capability
    ├── Tool
    └── Skill
```

定义：

> **Actor = 能持有 Intent 并在 Project Context 中主动发起 Action 的主体。**

```text
Actor
├── Identity
├── Role
├── Context Scope
├── Permission
├── Delegation
└── Capability Access
```

## 6.2 Agent

```text
Agent
├── Identity
├── Role
├── Instructions
├── Context Policy
├── Permission
├── Delegation
├── Skills
├── Tool Access
├── Model
└── Working State
```

`LLM ≠ Agent`。

Digger 更应围绕 Role，而不是巨型 Persona Prompt：

```text
Agent
 ↓
Role
 ↓
Responsibility
 ↓
Permission
 ↓
Capability
```

具体角色仍属于 Domain：

```text
Social Strategist
Writer
Researcher
SEO Editor
Video Producer
```

## 6.3 Delegation

Agent 更准确地说是 **Delegated Actor**：

```text
Delegation
├── Goal
├── Role
├── Scope
├── Permissions
├── Tools
├── Skills
├── Budget
├── Duration
└── Review Policy
```

因此：

```text
Autonomy
=
Delegated Scope
+ Permission
+ Risk Policy
+ Budget
+ Time
```

而不是一个 `autonomous=true/false`。

## 6.4 Permission

必须区分：

```text
Project Authority
= 什么信息/规则具有权威

Actor Permission
= 谁可以执行什么 Action
```

候选权限：

```text
Read
Create
Propose
Modify
Approve
Publish
Delete
Configure
```

关键：

```text
Create ≠ Propose ≠ Modify ≠ Approve
```

---

# 7. Capability

## 7.1 Tool

> **Tool = Actor 执行 Action 所使用的能力接口。**

Decision 属于 Actor，执行能力属于 Tool。

## 7.2 Skill

> **Tool = 能做什么；Skill = 如何把它做好。**

```text
Skill
├── Purpose
├── Instructions
├── Examples
├── Constraints
├── Recommended Tools
├── Input Schema
└── Output Schema
```

具体 Skill 由 Domain / Plugin 提供。

## 7.3 Workflow

Workflow Definition 不进入核心领域模型。

```text
Workflow = WHAT / WHEN
Role     = WHO
Skill    = HOW WELL
Tool     = HOW
Policy   = WHETHER ALLOWED
```

Digger 可以提供 Workflow Runtime，但 Workflow 本身属于 Project / Domain。

---

# 8. Knowledge Model

## 8.1 Source

Source 回答：

> 信息从哪里来？

```text
Source
├── File
├── Web Page
├── Dataset
├── Interview
├── Note
├── Image
├── Video
├── Previous Content
└── External System
```

```text
Source ≠ Knowledge ≠ Assertion ≠ Authority
```

## 8.2 Knowledge

Knowledge 回答：

> Project 能找到、理解和利用什么？

包括：

```text
Extracted Information
Note
Summary
Concept
Entity
Relationship
Observation-derived Information
Insight
```

## 8.3 Assertion

Assertion 是可判断的陈述：

```text
Assertion
├── Statement
├── Author
├── Confidence
├── Scope
├── Provenance
└── Status
```

状态可包括：

```text
Proposed
Accepted
Rejected
Superseded
```

Domain 解释：

```text
Writing → Claim
Social  → Audience Hypothesis
Site    → Content / SEO Hypothesis
```

## 8.4 Evidence 不是万能 Entity

压力测试表明，信息之所以成为 Evidence，是因为它相对于某个 Assertion 起支持或反驳作用。

因此：

```text
Knowledge
   │
   ├── supports ───────→ Assertion
   ├── contradicts ───→ Assertion
   └── contextualizes → Assertion
```

`Evidence` 是关系语义，而不是必须独立维护的一套万能对象。

## 8.5 Decision

```text
Decision
├── Subject
├── Decision
├── Rationale
├── Actor
├── Supporting Knowledge
├── Timestamp
├── Scope
└── Status
```

Decision 是 Domain State Change 的 provenance，不替代 Domain Object：

```text
Decision:
Increase architecture content
        ↓
Change:
Strategy.architecture
40% → 50%
```

---

# 9. Issue / Proposal / Intent / Candidate

四个概念必须严格区分。

## 9.1 Issue

> **Issue = 当前 Project State 中值得处理的问题。**

回答：

> **What is wrong / what needs handling?**

```text
Issue
├── Target
├── Type
├── Severity
├── Detected By
├── Supporting Knowledge
├── Description
├── Status
└── Resolution
```

例：

```text
Social  → failed publication / missing asset
Writing → unsupported claim / terminology conflict
Site    → stale page / broken link / external drift
```

## 9.2 Proposal

> **Proposal = Actor 对未来 Project 工作方向提出的建议。**

回答：

> **What should we do?**

```text
Proposal
├── Actor
├── Subject
├── Rationale
├── Context
└── Status
```

例如：

```text
写一篇 Project Memory 文章
增加 Project History 一章
新增 Coding Agent Category
新增 Comparison Content Program
```

Proposal 被拒绝时，Project State 可以完全不变化。

## 9.3 Intent

Intent 表示已经接受或主动发起、可以进入执行的目标：

```text
Proposal
   ↓ accepted
Intent
   ↓
Production Run
```

Human 也可以直接创建 Intent。

## 9.4 Candidate

> **Candidate = 尚未纳入 Official State 的具体 Project State Change。**

回答：

> **What exactly should change?**

因此：

```text
Proposal ≠ Candidate
```

典型链路：

```text
Issue / Observation / Idea
          ↓
       Proposal
          ↓
        Intent
          ↓
   Production Run
          ↓
        Change
          ↓
      Candidate
          ↓
        Commit
```

并非所有 Change 都必须成为 Candidate；低风险且有权限的 Change 可以自动 Commit。

---

# 10. Context Assembly

Agent Context 不等于 generic Memory Retrieval。

每个 Agent Task 必须回答：

```text
What am I working on?
What should I know?
What must I obey?
```

```text
                 Current Task
                      │
                      ↓
                Context Scope
                      │
          ┌───────────┼───────────┐
          ↓           ↓           ↓
      Knowledge    Authority    Working State
          │           │           │
       retrieve     resolve      inspect
          └───────────┼───────────┘
                      ↓
                Agent Context
                      ↓
                    Agent
```

Authority Resolution 可以考虑：

```text
Scope
Specificity
Priority
Validity
Conflict
```

但 v0.2 不应过早建设复杂通用规则引擎。

Domain 提供具体 Context：

```text
Social  → Brand / Audience / Channel / Strategy
Writing → Thesis / Terminology / Canon / Sources / Manuscript
Site    → Schema / Content Program / IA / Entries
```

---

# 11. Production Run

三轮压力测试确认 Production Run 应进入 Core。

> **Production Run = 围绕一个 Intent 组织起来、可追踪的逻辑生产执行。**

它不等于：

```text
Chat
Agent Session
OS Process
Single Tool Call
```

一个 Run 可以跨多个 Human / Agent Session：

```text
Production Run
├── Intent
├── Actors
├── Context
├── Inputs
├── Actions
├── Working State
├── Intermediate Results
├── Changes
├── Commits
└── Outcome
```

---

# 12. Working State / Result

## 12.1 Working State

Production 中经常存在：

- 不是 Official State；
- 不值得立即进入 Candidate；
- 又不能立即丢弃的中间状态。

例如：

```text
draft fragments
image_v1 / v2 / v3
temporary research results
alternative openings
partial build results
```

它们属于 Production Run 的 Working State。

## 12.2 Result ≠ Change

```text
Action
   ↓
Result
   ↓
Does this imply Project State change?
   ↓
Change
```

例如：

```text
Generate image
→ image.png                 Result

Attach image to Content
→ Project State changed     Change
```

---

# 13. Action

```text
Action
├── Actor
├── Intent
├── Target
├── Operation
├── Input
├── Context
├── Capability
└── Result
```

Core 不硬编码：

```text
write
render
crawl
publish
compile
deploy
```

这些 Operation 来自 Domain / Capability。

---

# 14. Change

Change 是 Digger Core 最稳定的抽象之一。

> **Change = Project State 从 S₀ 到 S₁ 的语义变化。**

```text
Change
├── Actor
├── Target
├── Operation
├── Before
├── After
├── Scope
├── Impact
├── Reversibility
├── Supporting Knowledge
└── Provenance
```

Domain 解释：

```text
Text Change
Timeline Change
Canon Change
Strategy Change
Schema Change
Image Change
```

Core 负责治理。

---

# 15. Change Set

大规模生产证明单个 Change 治理不够。

> **Change Set = 作为一个治理单元处理的一组相关 Changes。**

```text
Change Set
├── Intent
├── Changes[]
├── Scope
├── Summary
├── Impact
├── Validation
└── Governance
```

例如：

```text
Social:
Campaign update → 30 scheduled publications

Writing:
Terminology rename → 80 manuscript changes

Site:
Schema migration → 8,732 entries
```

Governance 可以对 Change Set 分区：

```text
Change Set
      ↓
Policy Evaluation
      ↓
┌──────────┬──────────┬──────────┐
↓          ↓          ↓
Auto       Review     Reject
```

---

# 16. Validation

Core 提供 Validation Mechanism，Domain 提供语义 Validator。

### Writing

```text
Canon consistency
Citation coverage
Unsupported claims
Structure consistency
```

### Social

```text
Brand rules
Platform limits
Asset requirements
```

### Site

```text
Schema validity
Broken relations
SEO rules
Build validity
```

Validation Failure 可以生成 Issue。

---

# 17. Impact Analysis

```text
Change Size ≠ Impact Size
```

例如：

```text
Change:
pricing_model
string → enum

Impact:
8,732 entries
```

因此 Risk 应考虑 Semantic Impact，而非简单 Diff Size。

```text
Core
└── Impact Engine
        ↑
        │
Domain
└── Dependency Provider
```

三个领域：

```text
Social:
Brand Rule → Scheduled Content

Writing:
Claim → Chapters / Figures / Conclusion

Site:
Schema → Entries / Templates / Pages
```

---

# 18. Policy / Governance

Governance 不等于 Human Approval。

```text
Change / Candidate / Change Set
        ↓
Policy Evaluation
        ↓
Auto Apply / Review / Reject
        ↓
Commit
```

Policy 可考虑：

```text
Actor
Role
Permission
Target
Operation
Scope
Impact
Risk
Reversibility
Confidence
Supporting Knowledge
Project Policy
```

原则：

```text
Low Reversibility
→ Higher Approval Requirement
```

例如：

```text
Edit draft paragraph       high reversibility
Publish website            medium reversibility
Send newsletter            low reversibility
```

---

# 19. Commit

> **Commit = 将一个或多个 Change 正式纳入 Project Official State 和 History。**

```text
Commit
├── Change / Change Set
├── Actor
├── Approver / Policy
├── Timestamp
├── Rationale
└── Provenance
```

Commit 是语义概念，不等同 Git Commit。

```text
Commit
≠ Revision
≠ Edition
≠ Deployment
```

这些仍属于 Domain。

---

# 20. Official State

必须区分：

```text
Working State
Candidate State
Official State
Published State
External State
```

两个维度彼此独立：

```text
Change Governance:
Candidate → Official

External Lifecycle:
Internal → Published / Distributed / Deployed
```

一个 Draft 完全可能已经是 Project 的 Official Internal State，但尚未对外发布。

---

# 21. External Interaction

Publication / Deployment / Distribution 保持 Domain-specific。

Core 只提供：

```text
External Interaction
├── External Action
├── External Reference
├── External State
└── Observation
```

## 21.1 External Action

> **External Action = Actor 在外部系统造成 Side Effect。**

```text
External Action
├── Destination
├── Payload Snapshot
├── Permission
├── Risk
├── Reversibility
├── Result
└── External Reference
```

映射：

```text
Social  → Publication
Site    → Deployment
Writing → Distribution
```

## 21.2 External State

```text
Internal State
Expected External State
Observed External State
External Drift
```

例如：

```text
Internal:  v12
Published: v10
Observed:  v10'
```

External Drift 经 Domain 解释后可以成为 Issue。

## 21.3 Observation

Observation 回答：

> **Project 在外部世界观察到了什么？**

例如：

```text
Post received 12,000 views
Reader complained about Chapter 4
External page hash changed
Search volume increased
```

Observation 不自动等于 Truth / Issue / Assertion。

```text
Observation
     ↓
Domain Interpretation
   ↙       ↘
Issue    Assertion
```

---

# 22. Feedback

Feedback 不作为万能 Core Object。

它是 Domain Interpretation：

```text
Social:
Observation → Engagement Feedback

Writing:
Observation → Reader / Editor Feedback

Site:
Observation → Analytics / SEO Signal
```

正确学习链：

```text
Observation
 ↓
Analysis
 ↓
Assertion
 ↓
Proposal / Learning Candidate
 ↓
Decision
 ↓
Change
 ↓
Commit
```

因此 Learning 本身也是 Project Change，需要同样的 Governance。

---

# 23. Project History / Causality

压力测试证明：

```text
History ≠ Commit History
```

Project 还需要保留：

```text
Proposal created / rejected
Issue detected / resolved
Decision made
Candidate rejected
Agent delegated
Production Run executed
External Action occurred
Observation received
```

目的不是普通 Activity Log，而是保留 **Project Causality**。

Digger 最终应能回答：

```text
Why does this exist?
Why was this changed?
What caused this production?
Which knowledge supported it?
Who proposed it?
Who or what approved it?
What happened afterward?
```

典型因果链：

```text
Observation
   ↓ caused
Assertion
   ↓ led to
Proposal
   ↓ accepted by
Decision
   ↓ initiated
Production Run
   ↓ produced
Change Set
   ↓ incorporated by
Commit
   ↓ externalized through
Publication / Distribution / Deployment
```

这可能成为 Digger 与普通 AI Workspace 最大的结构性差异之一。

---

# 24. Domain Contracts

Core 不理解 Chapter、Post、Schema、Canon 等对象，而由 Domain 通过 Contract/Adapter 提供能力。

## 24.1 Context Provider

```text
Task / Scope
    ↓
Domain Context Provider
    ↓
Relevant Domain State
```

## 24.2 Change Adapter

告诉 Core Domain State 发生了什么语义变化，而不要求 Core 理解对象内部语义。

## 24.3 Diff / Preview Provider

```text
Text Diff
Visual Diff
Timeline Diff
Structured Diff
Graph Diff
Semantic Diff
```

Diff 是 Domain 对 Core Change 的 View。

## 24.4 Validator

执行 Domain-specific semantic validation。

## 24.5 Dependency / Impact Provider

提供 Domain Dependency Graph，供 Core Impact Analysis 使用。

## 24.6 Authority Provider

暴露需要参与 Authority Resolution 的 Domain State。

## 24.7 Action / Capability Provider

暴露 Human / Agent 可以执行的领域 Action。

## 24.8 External Adapter

映射：

```text
Publication Adapter
Deployment Adapter
Distribution Adapter
```

到底层 External Action。

---

# 25. 明确不进入 Core 的概念

```text
Content
Document
Artifact
Post
Video
Page
Chapter
Scene
Manuscript
Brand
Audience
Campaign
Publication
Edition
Deployment
Analytics
Feedback
Canon
Claim
Citation
Schema
Collection
Content Program
Workflow Definition
```

不是因为这些概念不重要，而是：

> **它们的意义属于 Domain。**

---

# 26. 避免 Universal Artifact Trap

不应以：

```text
Artifact
├── type
├── content
└── metadata
```

作为中心，然后让：

```text
Post
Chapter
Video
Page
```

全部继承 Artifact。

Core 只需知道 Domain State 可以：

```text
be targeted by Action
be changed
be validated
be snapshotted
participate in Context
have impact relationships
```

---

# 27. 避免 Universal ProjectObject Trap

不要：

```text
ProjectObject
├── Brand
├── Chapter
├── Video
├── Citation
├── Page
└── Publication
```

应该：

```text
                    Project
                       │
               has Domain Model
                       │
          ┌────────────┼────────────┐
          ↓            ↓            ↓
        Social       Writing       Site
          │            │            │
        Brand         Work         Site
          │            │            │
     Domain Graph  Domain Graph  Domain Graph
```

---

# 28. End-to-End Core Loop

## 28.1 Internal Production Loop

```text
Intent
 ↓
Context Assembly
 ↓
Actor
 ↓
Production Run
 ↓
Action
 ↓
Result
 ↓
Change / Change Set
 ↓
Validation + Impact
 ↓
Candidate / Policy
 ↓
Commit
 ↓
Official State
```

## 28.2 External Evolution Loop

```text
Official State
 ↓
Snapshot
 ↓
External Action
 ↓
External State
 ↓
Observation
 ↓
Knowledge / Issue / Assertion
 ↓
Proposal
 ↓
Intent
 ↓
Production
 ↓
Change
 ↓
Commit
 ↓
Project Evolution
```

## 28.3 Combined Loop

```text
                         PROJECT
                            │
                            ↓
                         Intent
                            ↓
                     Context Assembly
                            ↓
                      Human / Agent
                            ↓
                     Production Run
                            ↓
                    Action → Result
                            ↓
                         Change
                            ↓
                       Change Set
                            ↓
                  Validation + Impact
                            ↓
                  Candidate + Policy
                            ↓
                          Commit
                            ↓
                     Official State
                            ↓
                         Snapshot
                            ↓
                    External Action
                            ↓
                     External World
                     ↙             ↘
              External State     Observation
                     ↓             ↓
                   Drift      Knowledge / Issue
                     └──────┬──────┘
                            ↓
                         Proposal
                            ↓
                       Project(t+1)
```

`Project History / Causality` 贯穿整个循环。

---

# 29. 三轮压力测试结论

## Case 1 — ByteBoy Social Project

```text
Brand / Audience / Strategy
→ Research
→ Topic Proposal
→ Content Production
→ Media Production
→ Variants
→ Publication
→ External Metrics
→ Assertion
→ Strategy Change
```

发现：

1. `Proposal ≠ Candidate`
2. Production 需要 Working State
3. Production Run 是独立 Runtime Boundary
4. `Content ≠ Publication`
5. `Publication ≠ External Action`
6. Observation 应进入 Knowledge，而不是建立第二套知识系统

## Case 2 — Project-centric AI Creator Book

```text
Work Intent
→ Thesis / Terminology
→ Research
→ Sources / Assertions
→ Outline
→ Manuscript
→ Validation
→ Revision
→ Edition
→ Distribution
→ Reader Feedback
```

发现：

1. Thesis / Terminology / Canon 强烈验证 Authority
2. Evidence 应建模为 Knowledge 与 Assertion 的关系
3. Issue 与 Proposal / Candidate 不同
4. Impact Analysis 对跨章节一致性至关重要
5. Revision 是 Domain；Commit 是 Core
6. History 必须包含 rejected Proposal / Candidate
7. Snapshot 与 Commit 不同

## Case 3 — AGC/OGC Site

```text
Site / Schema / Content Program
→ Agent Bulk Production
→ Change Set
→ Validation
→ Impact Analysis
→ Commit
→ Snapshot
→ Build / Deployment
→ External State
→ Drift / Analytics
→ Program Evolution
```

发现：

1. Production Run 是逻辑工作单元，不是 Agent Session
2. Bulk Production 需要 Change Set
3. Change Size 与 Impact Size 不同
4. Snapshot 是跨 Domain Core Concept
5. External Drift 经解释后成为 Issue
6. History 必须保存 Causal Chain
7. Governance 可以完全由 Policy 驱动，而不要求 Human Approval

---

# 30. Core Concept Validation Matrix

| Core Concept | Social | Writing | Site | v0.2 |
|---|:---:|:---:|:---:|---|
| Project | ✓ | ✓ | ✓ | Core |
| Actor | ✓ | ✓ | ✓ | Core |
| Delegation | ✓ | ✓ | ✓ | Core |
| Context | ✓ | ✓ | ✓ | Core |
| Source / Knowledge | ✓ | ✓ | ✓ | Core |
| Assertion | ✓ | ✓ | ✓ | Core |
| Decision | ✓ | ✓ | ✓ | Core |
| Authority | ✓ | ✓ | ✓ | Core |
| Issue | ✓ | ✓ | ✓ | Core |
| Proposal | ✓ | ✓ | ✓ | Core |
| Intent | ✓ | ✓ | ✓ | Core |
| Production Run | ✓ | ✓ | ✓ | Core |
| Action | ✓ | ✓ | ✓ | Core |
| Result | ✓ | ✓ | ✓ | Runtime |
| Working State | ✓ | ✓ | ✓ | Runtime |
| Change | ✓ | ✓ | ✓ | Core |
| Change Set | △ | ✓ | ✓✓ | Core |
| Validation | ✓ | ✓ | ✓ | Core mechanism |
| Impact | ✓ | ✓✓ | ✓✓ | Core mechanism |
| Candidate | ✓ | ✓ | ✓ | Core |
| Policy | ✓ | ✓ | ✓ | Core |
| Commit | ✓ | ✓ | ✓ | Core |
| Snapshot | ✓ | ✓ | ✓✓ | Core |
| External Action | ✓ | ✓ | ✓ | Core |
| External State | ✓ | ✓ | ✓ | Core |
| Observation | ✓ | ✓ | ✓ | Boundary concept |
| Project History | ✓ | ✓✓ | ✓✓ | Core |

---

# 31. Aggregate / Entity / Value Object / Event / Service 候选

> 本节只标记领域建模候选，不代表最终持久化设计。

## 31.1 Aggregate Candidates

Core：

```text
Project
Production Run
Change Set / Governance Unit
```

Domain：

```text
Social:
Brand
Content
Publication

Writing:
Work
Edition

Site:
Site
Content Program
Deployment
```

## 31.2 Entity Candidates

```text
Project
Actor
Agent
Delegation
Source
Knowledge Item
Assertion
Decision
Issue
Proposal
Production Run
Action
Change
Change Set
Candidate
Commit
Snapshot
External Action
External Reference
Observation
```

并非全部都必须独立持久化。

## 31.3 Value Object Candidates

```text
Project Scope
Context Scope
Permission
Risk
Impact
Reversibility
Authority Level
External Destination
State Reference
Provenance
```

## 31.4 Domain Event Candidates

```text
ProjectCreated
AgentDelegated
IssueDetected
ProposalCreated
ProposalAccepted
ProposalRejected
ProductionStarted
ProductionCompleted
ChangeProposed
CandidateAccepted
CandidateRejected
CommitCreated
SnapshotCreated
ExternalActionCompleted
ExternalDriftDetected
ObservationReceived
```

这不意味着 v0.2 决定采用 Event Sourcing。

## 31.5 Domain Service Candidates

```text
Context Assembly Service
Authority Resolution Service
Validation Service
Impact Analysis Service
Policy Evaluation Service
Change Governance Service
Snapshot Service
Project History / Causality Service
```

---

# 32. Digger Domain Architecture v0.2

```text
┌──────────────────────────────────────────────────────────┐
│                         DIGGER                           │
│                                                          │
│  ┌────────────────── PROJECT CORE ─────────────────────┐ │
│  │                                                    │ │
│  │ Project / Snapshot                                │ │
│  │ Actor / Role / Delegation / Permission            │ │
│  │ Source / Knowledge / Assertion / Authority        │ │
│  │ Issue / Proposal / Intent                         │ │
│  │ Production Run / Action / Working State           │ │
│  │ Change / Change Set / Candidate                   │ │
│  │ Validation / Impact / Policy                      │ │
│  │ Commit                                            │ │
│  │ External Action / State / Observation             │ │
│  │ Project History / Causality                       │ │
│  │                                                    │ │
│  └────────────────────────────────────────────────────┘ │
│                           │                              │
│                 Domain Contracts                        │
│                           │                              │
│       ┌───────────────────┼───────────────────┐          │
│       ↓                   ↓                   ↓          │
│ ┌───────────┐       ┌───────────┐       ┌───────────┐  │
│ │  SOCIAL   │       │  WRITING  │       │   SITE    │  │
│ │ Brand     │       │ Work      │       │ Site      │  │
│ │ Channel   │       │ Outline   │       │ IA        │  │
│ │ Strategy  │       │ Canon     │       │ Schema    │  │
│ │ Content   │       │ Claim     │       │ Program   │  │
│ │ Variant   │       │ Manuscript│       │ Entry     │  │
│ │ Publish   │       │ Edition   │       │ Deploy    │  │
│ └───────────┘       └───────────┘       └───────────┘  │
│       │                   │                   │          │
│       └───────────────────┼───────────────────┘          │
│                           ↓                              │
│                     AGENT RUNTIME                        │
│                 Role / Skill / Tool                      │
│                 Model / Execution                        │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

内部可以简写：

```text
Digger
=
Project Runtime
+
Creator Domains
+
Agent Runtime
```

这只是内部架构描述，不一定作为营销名称。

---

# 33. 六条稳定设计原则

## Principle 1 — Project is the boundary, not the content

Project 是长期持续工作的边界，而不是万能 Artifact。

## Principle 2 — Core owns transitions; Domain owns meaning

Core 管 Project 如何变化；Domain 管变化意味着什么。

## Principle 3 — Knowledge is not Authority

Agent 找到的信息不自动成为 Project Truth。

## Principle 4 — Proposal is not Candidate

```text
Proposal
= Should we do this?

Candidate
= Should we apply this concrete state change?
```

## Principle 5 — Agent output is not automatically Candidate

是否进入 Candidate 由：

```text
Permission
+ Policy
+ Impact
+ Risk
+ Reversibility
```

决定。

## Principle 6 — Project evolution must preserve causality

不仅保存：

```text
what changed
```

还要保存：

```text
why
based on what
proposed by whom
decided by whom or what policy
what production followed
what happened externally
what was learned afterward
```

---

# 34. 当前明确暂缓的问题

v0.2 不决定：

1. Physical persistence model
2. SQL / Graph / Document Storage
3. Event Sourcing
4. CRDT / Collaboration
5. Git integration
6. Vector DB
7. Agent Framework
8. LLM / Model Routing
9. UI Information Architecture
10. Editor Architecture
11. Workflow DSL
12. Plugin Packaging
13. Fine-grained Policy Language
14. Multi-project Relationships
15. Organization / Team Model
16. Billing / Budget Model
17. 最终 Aggregate 边界
18. Domain Contract 的具体 API

这些应从领域模型派生，而不是反过来塑造领域模型。

---

# 35. 下一阶段建议

v0.2 之后停止继续横向增加 Core Concept，进入更正式的领域建模。

## 35.1 Bounded Context

重点划分：

```text
Project Runtime
Knowledge / Authority
Agent Runtime
Change Governance
History / Causality
External Interaction
Social
Writing
Site
```

明确各 Context 的 ownership 与边界。

## 35.2 Lifecycle / State Machine

只为真正需要生命周期的 Core Object 建模：

```text
Proposal
Issue
Candidate
Production Run
Commit
Snapshot
External Action
```

不要给所有 Domain Object 强加统一状态机。

## 35.3 Domain Contracts

进一步正式定义：

```text
Context Provider
Authority Provider
Change Adapter
Diff / Preview Provider
Validator
Dependency / Impact Provider
Action Provider
External Adapter
```

完成后，再进入 Technical Architecture。

---

# 36. 最终模型

Digger v0.2 可以用下面一段话概括：

> A Project is a persistent creator working boundary. Human and Agent Actors operate inside it. They receive Context assembled from Knowledge, Authority and Working State. Intent drives Production Runs. Production Runs execute Actions and produce Results. Results may imply Changes. Changes are validated, impact-analyzed and governed. Accepted Changes become Official State through Commit. Snapshots stabilize selected Project State. External Actions publish, distribute or deploy snapshots. The Project observes the external world. Observations become Knowledge, Issues or Assertions. Those may lead to Proposals, new Intent and new Changes. Project History preserves the causal chain. Domains define what all of this actually means.

更短：

```text
                    PROJECT
                       │
                       ↓
              Understand Context
                       ↓
                 Human / Agent
                       ↓
                    Intent
                       ↓
                Production Run
                       ↓
                    Change
                       ↓
                   Governance
                       ↓
                    Commit
                       ↓
                Official State
                       ↓
                   Snapshot
                       ↓
                External World
                       ↓
                  Observation
                       ↓
               Learn / Propose
                       ↓
                   PROJECT'
```

这就是当前 **ByteBoy Digger** 的领域模型核心。

---

# Appendix A — Vocabulary

| Term | Meaning |
|---|---|
| Project | 持久化的 Creator 工作边界 |
| Domain | Project 的专业领域语义模型 |
| Actor | 能主动发起 Action 的 Human / Agent |
| Delegation | 授予 Agent 的目标、范围和权限 |
| Tool | 可执行能力 |
| Skill | 如何把工作做好的 know-how |
| Source | 信息来源 |
| Knowledge | Project 可理解和利用的信息 |
| Assertion | 可判断的陈述 |
| Authority | 对未来工作具有约束力的 Project State |
| Issue | 当前状态中值得处理的问题 |
| Proposal | 对未来工作/方向的建议 |
| Intent | 已进入执行阶段的目标 |
| Production Run | 围绕 Intent 的可追踪逻辑生产执行 |
| Action | Actor 对 Target 执行的操作 |
| Result | Action 执行结果 |
| Working State | Production 中间状态 |
| Change | Project State 的语义变化 |
| Change Set | 作为一个治理单元的一组 Changes |
| Candidate | 尚未纳入 Official State 的具体 Change |
| Validation | Domain 正确性验证 |
| Impact | Change 的语义影响 |
| Policy | 决定权限与治理路径的规则 |
| Commit | 将 Change 正式纳入 Official State |
| Official State | 已被 Project 接受的内部状态 |
| Snapshot | 对某个 State 的不可变引用 |
| External Action | 在外部系统产生 Side Effect 的动作 |
| External State | Project 外部的状态 |
| Observation | 从外部世界观察到的信息 |
| Provenance | 信息/状态的来源与因果上下文 |
| Project History | Project 演化的因果历史 |

---

# Appendix B — Core vs Domain

```text
CORE                               DOMAIN

Project                            Brand / Work / Site
Actor                              Writer / SEO Editor / Strategist
Authority                          Canon / Brand Rule / Schema Rule
Context Assembly                   Audience / Manuscript / Site Context
Issue                              Broken Link / Unsupported Claim
Proposal                           Topic / Chapter / Category Proposal
Production Run                     Writing / Video / Bulk Generation
Action                             Render / Write / Crawl / Deploy
Change                             Text / Canon / Schema Change
Change Set                         Migration / Bulk Revision
Validation                         Citation / Brand / Schema Validation
Impact                             Chapter / Campaign / Page Dependencies
Candidate                          Domain-specific proposed modification
Commit                             Official domain state transition
Snapshot                           Edition / Publication / Deployment source
External Action                    Publish / Distribute / Deploy
Observation                        Metrics / Reader Response / SEO Signal
History                            Domain-specific causal chain
```

---

**End of ByteBoy Digger Domain Model v0.2**
