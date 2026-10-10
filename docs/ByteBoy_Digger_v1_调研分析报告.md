# ByteBoy Digger v1 调研分析报告

> **产品名称**：ByteBoy Digger\
> **品类定位**：AI Creator\
> **报告版本**：Research v1.0\
> **日期**：2026-09-30\
> **文档性质**：调研分析报告，非产品需求规格书（PRD）

------------------------------------------------------------------------

## 1. 执行摘要

ByteBoy Digger 的研究起点不是"再做一个 AI
写作工具"，也不是把文本、图片、视频生成能力集中到一个万能 Workspace
中，而是把 **Project-centric**
作为设计哲学，研究在内容创作领域中，一个长期存在、持续积累并不断演化的
Project 应该如何被 Human、Agent 与 Tool 共同维护。

本轮调研覆盖三个一级领域：

1.  **Social Creator / 自媒体内容**
2.  **Long-form / 长篇写作**
3.  **AGC / OGC Website / 非 UGC 内容网站**

调研分三轮进行：

-   **第一轮：AI-native 完整产品样本**，研究 Agent-native Creator
    已经出现了哪些新的产品结构；
-   **第二轮：领域补全样本**，补齐 Distribution、Video
    Production、Research Writing、Programmatic Content、SEO、CMS
    等关键环节；
-   **第三轮：成熟传统产品控制组**，区分"领域本来就需要的能力"与"Agent
    时代真正新增的结构"。

本轮最重要的研究结论是：

> **不要只优化 Output，要优化产生 Output 的 Project。**

文本、图片、视频、网页等 Output
会被发布、消费和替换；真正具有长期价值的是生产这些 Output 的
Project。Project
会持续积累知识、结构、规则、决策、技能、工作流、模板、素材、反馈和生产历史，并因此逐渐形成更强的生产能力。

因此，Digger 不应被理解为：

``` text
Editor + Chat + Knowledge Base
```

也不应被理解为：

``` text
Text Generator + Image Generator + Video Generator
```

更接近：

``` text
Project
  + Domain Model
  + Human / Agent / Tool
  + Production
  + Governance
  + Publication
  + Feedback
```

三个领域可以共享 Project-centric 方法论和部分 Agent
基础设施，但它们拥有不同的领域对象、工作节奏和专业界面。Digger
不应通过一个万能对象模型强迫三个领域趋同。

------------------------------------------------------------------------

## 2. 研究背景：Project-centric 与产品边界

### 2.1 Project-centric 是设计哲学，不是一个万能产品

本研究首先区分三个层次：

``` text
Project-centric Philosophy
        ↓
Project Methodology
        ↓
Domain Products
   ├── ByteBoy Dozer — AI Coder
   └── ByteBoy Digger — AI Creator
```

Project-centric 关注的是：

> 如何让一个长期存在的 Project
> 持续吸收知识、工具、流程、经验和反馈，从而越来越有能力产生高质量
> Output。

具体产品只是这一方法论在不同领域中的实践。

### 2.2 Dozer 与 Digger 的边界

**ByteBoy Dozer** 面向以代码为核心的软件生产 Project。

**ByteBoy Digger** 面向以文字、图片、音频、视频和网页内容为核心的
Creator Project。

Dozer 可以被用来开发 Digger，但这并不意味着 Dozer 包含 Digger
的领域能力。两者共享设计哲学，却不应共享全部领域模型。

这一边界非常重要。ByteBoy 不应演化成"什么都能做"的超级 Workspace：

> 到处是路，反而也就没有路。

### 2.3 Digger 的横向研究范围

``` text
ByteBoy Digger / AI Creator
│
├── Social Creator
│   └── 自媒体、社交媒体、多平台内容
│
├── Long-form
│   └── 小说、书籍、研究报告、专业长文
│
└── AGC / OGC Website
    └── 编辑型、机构型、AI 生成型内容网站
```

本轮明确不以 UGC 社区为核心研究对象。UGC
平台的用户、社区治理、社交关系、推荐系统等问题会显著改变领域模型。

------------------------------------------------------------------------

# 3. 调研方法

## 3.1 三轮研究结构

### 第一轮：AI-native 完整产品

重点回答：

-   Project 如何组织？
-   Agent 如何参与？
-   长期知识保存在哪里？
-   Human 与 Agent 如何共同修改正式内容？
-   生产、审核和发布如何形成闭环？

核心样本：

-   Social：Easel
-   Long-form：WordBird、NarraLume
-   Website：SiteSpec、Caelo CMS

### 第二轮：领域能力补全

重点补齐第一轮没有充分覆盖的专业子领域：

-   Social Operations：Postiz
-   Video Production：OpenMontage
-   Research Writing：Synthetix
-   Programmatic Content / SEO：OpenSEO
-   Mature CMS Infrastructure：Payload

### 第三轮：成熟领域控制组

目的不是研究 AI，而是回答：

> 如果没有 AI，这个领域经过多年实践后仍然留下了哪些稳定对象和工作流？

控制组包括：

-   Social：Mixpost、Socioboard
-   Long-form：Scrivener、novelWriter、bibisco
-   CMS：Payload、Directus

这种对照可以避免把"成熟领域能力 + LLM"误判为 Agent-native 创新。

------------------------------------------------------------------------

# 4. 第一领域：Social Creator

## 4.1 核心判断：长期根对象更接近 Account / Brand

Easel 是本轮最接近完整 AI Creator 产品形态的 Social 样本。

其重要启示不是某个生成能力，而是它把长期存在的信息组织在 Creator /
Account Profile 周围，包括：

-   Identity
-   Audience
-   Voice / Style
-   Platforms
-   Preferences / Boundaries
-   Long-term Memory
-   Historical Performance

而单次 Content Project 则包含：

-   Source
-   Research
-   Idea
-   Script / Copy
-   Assets
-   Intermediate Artifacts
-   Platform Variants
-   Deliverables

因此 Social Creator 更接近：

``` text
Account / Brand
│
├── Identity
├── Audience
├── Voice / Style
├── Platforms
├── Preferences / Boundaries
├── Long-term Memory
│
├── Content Strategy
│   ├── Topics
│   ├── Series
│   ├── Campaigns
│   └── Calendar
│
└── Content Projects
    ├── Source
    ├── Research
    ├── Idea
    ├── Script / Copy
    ├── Assets
    ├── Intermediate Artifacts
    └── Variants
         ↓
      Publication
         ↓
      Performance
         ↓
      Attribution
         ↓
   Account / Strategy
```

其基本规律可以概括为：

> **一条内容会结束，一个账号或品牌却持续经营。**

因此 Social Project 真正持续优化的是：

``` text
Account / Brand
      ↓
Content Strategy
      ↓
Production System
      ↓
Audience Understanding
```

Content 是这一系统持续产生的 Output。

## 4.2 Social Creator 的三个基本阶段

结合 Easel 与 Postiz，可以把 Social Creator 拆为三个大的子域：

``` text
① Content Intelligence
   Discover / Research / Idea / Strategy

② Content Production
   Text / Image / Audio / Video

③ Content Operations
   Channel / Schedule / Publish / Analytics
```

传统 Social Management 产品往往从第 ③ 层做深，而 AI-native Creator
正在向第 ①、② 层扩展，并把 Analytics 再反馈到下一轮生产。

------------------------------------------------------------------------

## 4.3 Publication 必须与 Content 分离

Postiz、Mixpost 等成熟 Social Operations
产品说明，"发布"本身具有独立生命周期。

不能简单设计为：

``` text
Content.publish()
```

更合理的是：

``` text
Content #37
├── WeChat Publication
├── Xiaohongshu Publication
├── X Publication
├── LinkedIn Publication
└── YouTube Publication
```

每个 Publication 都可能具有自己的：

-   Platform / Channel
-   Content Variant
-   Schedule
-   Publication Status
-   URL / External ID
-   Analytics
-   Failure / Retry
-   Feedback

因此：

> **Content ≠ Publication**

Publication 应被视为 Creator 中值得重点研究的一等领域对象。

------------------------------------------------------------------------

## 4.4 Video 不是普通 Asset

OpenMontage 暴露了另一个重要问题。

视频生产并不是：

``` text
Prompt → MP4
```

而更接近：

``` text
Video Project
│
├── Intent
├── References
├── Research
├── Script
├── Scene Plan
│
├── Assets
│   ├── Footage
│   ├── Image
│   ├── Voice
│   ├── Music
│   └── Graphics
│
├── Timeline / Composition
├── Subtitle
├── Style
├── Render
└── QA
```

OpenMontage 的架构还表现出清晰的能力分层：

``` text
Tools
  ↓
能做什么

Pipeline Definitions
  ↓
生产过程是什么

Skills
  ↓
应该怎么做好

Schemas
  ↓
中间结果必须长什么样

Styles
  ↓
视觉语言是什么

Renderer / Remotion
  ↓
如何形成最终成品
```

这说明复杂 Media 不应被简单压缩成一个附件类型。

Digger 需要考虑：

> **复杂内容是否应该成为嵌套 Project / Subproject？**

例如：

``` text
Social Content Project
        │
        └── Video Production Project
```

这一点目前应保留为领域模型假设，而不是立即固化为技术结构。

------------------------------------------------------------------------

## 4.5 Social Creator 的成熟控制组

Mixpost、Socioboard 等传统产品已经验证了一批稳定对象：

``` text
Workspace / Brand
│
├── Social Accounts
├── Team
├── Media Library
├── Templates
│
├── Content
│   └── Platform Variants
│
├── Calendar
├── Queue
├── Schedule
├── Publication
└── Analytics / Reports
```

这些对象本身不是 AI-native 创新。

Agent 时代真正改变的是生产端：

``` text
传统：

Human
  ↓
Create Content
  ↓
Operations
  ↓
Publish


Agent-native：

Project Knowledge
      ↓
Agent Research
      ↓
Idea / Strategy
      ↓
Agent Production
      ↓
Text / Image / Video
      ↓
Review / Policy
      ↓
Operations
      ↓
Publish
      ↓
Analytics
      ↓
Project Learning
```

------------------------------------------------------------------------

# 5. 第二领域：Long-form

## 5.1 Long-form 不能只理解成"大号文本编辑器"

WordBird、NarraLume、Scrivener、novelWriter、bibisco
都显示出一个共同事实：

> 长篇写作的核心不是 Document，而是 Work / Writing Project。

成熟模型通常包含：

``` text
Writing Project
│
├── Binder / Structure
│   ├── Manuscript
│   │   ├── Part
│   │   ├── Chapter
│   │   └── Scene / Section
│   │
│   ├── Research
│   └── Notes
│
├── Outline
├── Metadata
├── Collections
├── Snapshots / Versions
└── Compile / Export
```

因此 Binder、Outline、Research、Snapshot、Compile 等能力并不是 AI
时代的新发明。

Agent-native 产品需要解决的是 Agent 如何安全、长期地参与这个已经成熟的
Project。

------------------------------------------------------------------------

## 5.2 Narrative Writing：Canon 是核心约束

WordBird 和 NarraLume 都把 Story
Bible、Facts、Characters、Timeline、World、Decisions
等信息从普通笔记中提升出来。

可以形成：

``` text
Narrative Project
│
├── Author Intent
│
├── Story Bible
│   ├── Characters
│   ├── World
│   ├── Timeline
│   ├── Relations
│   ├── Canon Facts
│   └── Decisions
│
├── Outline
│
├── Manuscript
│   ├── Volume
│   ├── Chapter
│   └── Scene
│
├── Versions
├── Comments
├── Reviews
└── Candidates
```

这里最重要的是 **Canon**。

例如：

``` text
“主人公左眼失明”
```

一旦被作者确认并进入 Canon，它就不再只是一个历史 Memory，而是：

> **未来创作必须服从的 Project Truth。**

因此 Narrative Project 的基本约束链条是：

``` text
Knowledge
   ↓
Decision
   ↓
Canon
   ↓
Constraint
   ↓
Future Creation
```

------------------------------------------------------------------------

## 5.3 Research Writing：Evidence 取代 Canon 成为核心

Synthetix 补齐了非虚构长篇这一重要分支。

其工作模式更接近：

``` text
Sources
│
├── Documents
├── Papers
├── Reports
├── Web
├── Notes
└── Data
       ↓
Knowledge Base
       ↓
┌─────────────────────┐
│ Evidence / Claims   │
│ Concepts            │
│ Relations           │
│ References          │
└──────────┬──────────┘
           ↓
        Outline
           ↓
       Manuscript
           │
      ┌────┴────┐
      ↓         ↓
   Section    Section
      ↓         ↓
   Sources    Sources
      └────┬────┘
           ↓
       References
```

因此 Long-form 至少需要区分：

``` text
Long-form
│
├── Narrative Writing
│   ├── Novel
│   ├── Biography
│   └── Narrative Non-fiction
│
└── Research Writing
    ├── Book
    ├── Report
    ├── Research
    ├── Technical Writing
    └── Consulting
```

两者的核心约束不同：

``` text
Narrative Writing
核心约束 = Canon

Research Writing
核心约束 = Evidence
```

小说 Agent 最怕前后设定冲突。

研究型写作 Agent 最怕输出没有可靠来源支持的结论。

因此：

``` text
Fiction Truth
→ Character / World / Timeline / Canon

Research Truth
→ Source / Evidence / Claim / Citation
```

这意味着 Source、Claim、Evidence、Citation 在 Research Writing
中应该是一等领域对象，而不是普通附件。

------------------------------------------------------------------------

## 5.4 Candidate：AI Output 不应天然等于 Manuscript

NarraLume 等 AI-native 长篇工具出现了一个值得保留的结构：

``` text
AI Output
    ↓
Candidate
    ↓
Diff / Evidence / Review
    ↓
Decision
   ↙       ↘
Accept    Reject
  ↓
Official Work
```

传统模式是：

``` text
Human → Manuscript
```

Agent-native 模式变成：

``` text
Human Intent
     ↓
Project Context
     ↓
Agent
     ↓
Candidate
     ↓
Diff / Review
     ↓
Decision
     ↓
Manuscript
```

因此真正的新能力并不是"AI 能写章节"，而是：

-   Agent Context
-   Candidate
-   Diff
-   Project Truth
-   AI Change Governance

------------------------------------------------------------------------

# 6. 第三领域：AGC / OGC Website

## 6.1 Website 的根对象不是 Article，而是 Site System

SiteSpec 表明，内容网站的长期 Project 更接近：

``` text
Site Project
│
├── Information Architecture
│
├── Design System
│   ├── Tokens
│   ├── Components
│   └── Shell
│
├── Content Model
│   ├── Schemas
│   ├── Collections
│   ├── Relations
│   └── Taxonomy
│
├── Content
│   ├── Entries
│   ├── Assets
│   └── References
│
├── Page Specs
│   ├── Archetypes
│   ├── Routes
│   └── Sections
│
└── Rendering / Publication
```

网站 Creator 的核心不是"AI 写文章"，而是：

> **定义并长期维护一个可以持续生产和组织网页的 Content System。**

------------------------------------------------------------------------

## 6.2 Content Program：AGC 的关键对象

OpenSEO 进一步说明，AGC / Programmatic Content 的生产逻辑不是：

``` text
Prompt → Article
```

而是：

``` text
Business Profile
       ↓
Topic / Keyword Space
       ↓
Content Strategy
       ↓
Calendar / Plan
       ↓
Article / Page Structure
       ↓
Typed Blocks
       ↓
Content
       ↓
Analyze
       ↓
Publish
```

因此网站长期 Project 可能存在一个非常重要的对象：

> **Content Program**

Content Collection 描述"已经有什么"。

Content Program 则描述：

-   为什么生产；
-   面向什么 Audience；
-   覆盖什么 Topic Space；
-   下一步生产什么；
-   用什么 Schema / Template；
-   如何避免重复；
-   如何互链；
-   如何更新；
-   如何根据 SEO / Analytics 调整下一轮生产。

其典型形态是：

``` text
Schema
  +
Template
  +
Data / Knowledge
  +
Generation Rules
  ↓
10 Pages
100 Pages
10,000 Pages
```

因此 AGC 的规模化能力来自 **Program**，而不是简单的 Batch Generation。

------------------------------------------------------------------------

## 6.3 成熟 CMS 已经解决了大量"内容治理"问题

Payload、Directus 等成熟 CMS 已经形成稳定结构：

``` text
Collections
Globals
Fields
Relationships
Media

Versions
Drafts
Autosave
Preview
Scheduled Publishing

Access Control
Hooks
API
```

典型内容生命周期已经是：

``` text
Published
   │
   └──→ Draft / Version
              ↓
            Edit
              ↓
           Preview
              ↓
           Review
              ↓
           Publish
              ↓
          Published
```

因此：

> **Draft → Review → Publish 并不是 Agent-native 创新。**

Agent 时代真正改变的是：

``` text
过去：
Human edits Draft

现在：
Human + Agent edit Draft
```

甚至进一步：

``` text
Agent becomes a large-scale Draft Producer
```

这会使 Change Governance、Evidence、Review、Policy、Batch Operation
等问题的重要性显著提高。

------------------------------------------------------------------------

# 7. 三大领域的领域地图

经过三轮研究，三个一级领域可以初步拆成以下核心子领域。

  -----------------------------------------------------------------------
  一级领域                核心子领域              主要领域对象 / 问题
  ----------------------- ----------------------- -----------------------
  Social Creator          Content Intelligence    Topic / Audience /
                                                  Strategy

  Social Creator          Text Production         Post / Article

  Social Creator          Visual Production       Image / Design

  Social Creator          Video Production        Script / Scene /
                                                  Timeline / Assets

  Social Creator          Content Operations      Channel / Publication /
                                                  Schedule

  Social Creator          Audience Intelligence   Analytics / Attribution
                                                  / Learning

  Long-form               Narrative Writing       Manuscript / Canon /
                                                  Story Bible

  Long-form               Research Writing        Source / Claim /
                                                  Evidence / Citation

  Long-form               Knowledge Management    Notes / Concepts /
                                                  Relations

  Long-form               Structure               Outline / Chapter /
                                                  Section

  Long-form               Revision                Candidate / Diff /
                                                  Version

  Long-form               Publishing              Edition / Compile /
                                                  Export

  AGC/OGC Website         Information             Taxonomy / Relation /
                          Architecture            Route

  AGC/OGC Website         Content Modeling        Schema / Collection /
                                                  Field

  AGC/OGC Website         Content Program         Topic / Keyword /
                                                  Generation Rule

  AGC/OGC Website         Production              Entry / Page / Asset

  AGC/OGC Website         Site System             Template / Component /
                                                  Design System

  AGC/OGC Website         Content Governance      Draft / Version /
                                                  Review

  AGC/OGC Website         Publishing              Build / Deploy /
                                                  Schedule

  AGC/OGC Website         Growth                  SEO / Analytics /
                                                  Update
  -----------------------------------------------------------------------

------------------------------------------------------------------------

# 8. 三个领域不能被强行统一

## 8.1 根对象不同

  ------------------------------------------------------------------------
                    Social Creator    Long-form         AGC/OGC Website
  ----------------- ----------------- ----------------- ------------------
  根对象            Account / Brand   Work / Book       Site

  长期目标          持续经营          完成并演化作品    持续运营内容系统

  核心单位          Content / Post    Chapter / Scene / Entry / Page
                                      Section           

  核心结构          Calendar / Series Outline /         Schema /
                    / Campaign        Manuscript        Collection /
                                                        Relation

  长期知识          Audience /        Bible / Sources   Business / Data /
                    Profile                             Knowledge

  强约束            Brand / Platform  Canon / Evidence  Schema / Contract
                    Rules                               

  生产特点          高频、多模态      长周期、深度      批量、结构化

  发布              Multi-platform    Edition / Export  Site Publication
                    Publication                         

  反馈              Engagement        Review / Reader   Traffic / SEO /
                                                        Conversion
  ------------------------------------------------------------------------

## 8.2 时间形态不同

``` text
Social
循环型 Project

Produce
  ↓
Publish
  ↓
Learn
  ↓
Produce
```

``` text
Long-form
收敛型 Project

Research
  ↓
Write
  ↓
Revise
  ↓
Complete / Edition
```

``` text
Website
演化型 Project

Model
  ↓
Generate
  ↓
Publish
  ↓
Measure
  ↓
Update
```

这是 Digger 不应采用"一套 Workflow 适配所有 Creator"的重要原因。

------------------------------------------------------------------------

# 9. 成熟领域能力与 Agent-native 增量

第三轮控制组使两类能力可以被清楚地区分。

  -------------------------------------------------------------------------------
  领域                    AI 前成熟模型           Agent-native 增量
  ----------------------- ----------------------- -------------------------------
  Social                  Account / Content /     Research
                          Calendar / Queue /      Agent、Idea、跨模态生产、自动
                          Publication / Analytics Variant、反馈学习

  Long-form               Project / Binder /      Project Truth、Agent
                          Outline / Research /    Context、Candidate、Agent
                          Manuscript / Snapshot / Revision、Evidence / Canon
                          Compile                 Governance

  Website                 Schema / Collection /   Content Program、Agent
                          Relation / Draft /      Author、批量生成、Agent
                          Version / Preview /     Maintenance、Programmatic SEO
                          Publish                 
  -------------------------------------------------------------------------------

这一对照带来的原则是：

> Digger
> 应复用成熟领域知识，而不是为了"AI-native"重新发明已经被验证的领域模型。

真正值得重点设计的是 Agent 如何进入这些模型。

------------------------------------------------------------------------

# 10. 跨领域共同模式

尽管三个领域差异明显，三轮研究仍然出现了一组稳定的共同结构。

``` text
                         PROJECT
                            │
       ┌────────────────────┼────────────────────┐
       ↓                    ↓                    ↓
   Knowledge            Structure             Rules
       │                    │                    │
       └────────────────────┼────────────────────┘
                            ↓
                         Intent
                            ↓
                         Context
                            ↓
                  Human + Agent + Tool
                            ↓
                       Production
                            ↓
                        Candidate
                            ↓
                    Review / Policy
                            ↓
                    Official State
                            ↓
                       Publication
                            ↓
                        Feedback
                            ↓
                         Learning
                            │
                            └────────→ PROJECT
```

这应被视为 **Creator 方法论模型候选**，而不是已经确定的数据模型。

------------------------------------------------------------------------

# 11. Project Truth：比普通 Memory 更重要的候选概念

三大领域都存在某种"后续生产必须服从的信息"，只是形式不同。

``` text
Social
├── Brand Rules
├── Audience Knowledge
├── Platform Rules
└── Decisions

Narrative Writing
├── Author Intent
├── Canon
├── Story Bible
└── Decisions

Research Writing
├── Source
├── Evidence
├── Claim
└── Citation

Website
├── Schema
├── Site Spec
├── Design System
└── Content Rules
```

这些信息与普通 Memory 不同。

普通 Memory 更接近：

> 我们以前知道、讨论或做过什么。

Project Truth 更接近：

> 在当前 Project
> 中，后续生产应该把什么视为已经确定的事实、约束、规则和决定。

因此 Agent 需要知道的不只是"更多上下文"，而是：

``` text
什么只是 Source
什么只是历史
什么只是 Suggestion
什么是 Preference
什么已经被决定
什么是当前 Fact
什么是 Rule
什么允许修改
什么不能自动修改
```

这使问题从 **Memory Management** 上升为：

> **Project Knowledge Governance**

"Project Truth"目前是研究阶段提出的概念，需要在后续领域建模中继续验证。

------------------------------------------------------------------------

# 12. Candidate：Proposal 与 Official State 分离

多个互相独立的 AI-native 产品出现了类似结构：

``` text
Agent Proposal
      ↓
Mutable / Reviewable State
      ↓
Decision / Policy
      ↓
Official Project State
```

具体表现可能是：

-   Candidate
-   Draft
-   Snapshot
-   Diff
-   Proposed Change

其核心不是"所有修改必须人工审批"。

更一般的模型应该是：

``` text
Agent Action
     ↓
Candidate
     ↓
Diff / Preview / Evidence
     ↓
Policy
  ┌──┼───────────┐
  ↓  ↓           ↓
Auto Human      Reject
  │  Review
  └──┬───────────┘
     ↓
Official State
```

低风险修改可以自动接受，中风险修改可以经过 Policy，高风险修改进入 Human
Review。

因此真正稳定的抽象可能是：

> **Proposal / Candidate 与 Official State 分离。**

这比单纯强调 Human-in-the-loop 更普适。

------------------------------------------------------------------------

# 13. Agent 成为 Project Actor

传统内容软件中的主要 Actor 是 Human。

``` text
Project
├── Human A
├── Human B
└── Human C
```

Agent-native Creator 变成：

``` text
Project
├── Human
├── Agent
├── Agent
└── Tools
```

Agent 不再只是一次性的文本生成 API，而可能长期参与：

-   Research
-   Planning
-   Production
-   Editing
-   Review
-   Maintenance
-   Analysis
-   Learning

这使以下问题成为产品核心：

-   Agent 当前可以看到什么？
-   Agent 可以修改什么？
-   Agent 可以调用哪些 Tool？
-   Agent 当前代表什么 Role？
-   哪些动作需要 Review？
-   Agent 的修改如何被追踪？
-   Agent 如何理解 Project Truth？
-   多 Agent 如何共享 Project Context？

------------------------------------------------------------------------

# 14. Context Assembly 成为核心基础能力

传统软件通常依赖 Human 自己理解屏幕上的上下文：

``` text
Human 看 UI
    ↓
Human 自己理解 Context
```

Agent 则必须由系统显式构造上下文：

``` text
Project
   ↓
Context Assembly
   ↓
Agent
```

Context 可能来自：

-   当前工作对象；
-   当前选区；
-   Project Knowledge；
-   Project Truth；
-   Source / Evidence；
-   Structure；
-   History；
-   Relevant Assets；
-   Publication Feedback；
-   Tool State。

因此 Digger 的核心问题之一不是：

> 如何拥有最大的 Context Window？

而是：

> **如何给当前 Agent 提供恰当、权威层级正确、与任务相关的 Project
> Context？**

------------------------------------------------------------------------

# 15. Feedback 使 Project 具备演化能力

传统 Creator 软件中的 Analytics 通常止于报表：

``` text
Analytics
   ↓
Human 看报表
   ↓
Human 自己思考
   ↓
下一轮生产
```

Agent-native Creator 可以形成：

``` text
Publication
     ↓
Feedback
     ↓
Analysis
     ↓
Learning Candidate
     ↓
Decision
     ↓
Project Knowledge / Strategy / Rules
     ↓
Next Production
```

因此 Feedback 不只是一个 Dashboard。

它有机会成为 Project Evolution 的输入。

需要注意：Feedback 不应未经治理直接改写 Project
Truth，否则短期指标可能破坏长期定位。因此更合理的是：

``` text
Feedback
   ↓
Learning Candidate
   ↓
Review / Policy
   ↓
Project Update
```

------------------------------------------------------------------------

# 16. Digger Creator Domain Map v0.1

根据当前研究，可以形成如下候选地图：

``` text
                    BYTEBOY DIGGER
                      AI CREATOR

                        PROJECT
                           │
          ┌────────────────┼────────────────┐
          ↓                ↓                ↓
      Knowledge        Structure          Rules
          │                │                │
          └────────────────┼────────────────┘
                           ↓
                        Context
                           ↓
                  Human + Agent + Tool
                           ↓
                      Production
                           ↓
                       Candidate
                           ↓
                    Review / Policy
                           ↓
                    Official State
                           ↓
                      Publication
                           ↓
                       Feedback
                           ↓
                       Learning
                           │
                           └──────→ PROJECT
```

在这一共同方法论下面，存在不同的领域 Project Experience：

``` text
Digger
│
├── Social Project
│   ├── Account / Brand
│   ├── Strategy
│   ├── Campaign / Series
│   ├── Calendar
│   ├── Content
│   ├── Media Production
│   ├── Publication
│   └── Analytics
│
├── Writing Project
│   ├── Work
│   ├── Binder / Structure
│   ├── Research
│   ├── Outline
│   ├── Canon / Evidence
│   ├── Manuscript
│   ├── Candidate / Revision
│   └── Edition
│
└── Site Project
    ├── Site
    ├── Information Architecture
    ├── Schema / Taxonomy
    ├── Content Program
    ├── Collection / Entry
    ├── Template / Design System
    ├── Publication
    └── Analytics / SEO
```

这一结构强调：

> **共享 Project 哲学和 Agent
> Infrastructure，但不强迫三个领域共享同一种工作界面、领域对象和生产流程。**

------------------------------------------------------------------------

# 17. "输出会消耗，Project 会积累"

三轮调研最终都指向同一个长期价值模型。

Project 真正积累的不是 Output 数量，而是：

``` text
Knowledge
+ Rules
+ Structure
+ Decisions
+ Skills
+ Workflows
+ Templates
+ Assets
+ Feedback
+ Production History
```

可以表达为：

``` text
Project(t0)
   ↓
Production
   ↓
Output
   ↓
Publication
   ↓
Feedback
   ↓
Learning / Decision
   ↓
Project(t1)
```

并期望：

``` text
Capability(Project t1) > Capability(Project t0)
```

这可能是 Project-centric Creator 与普通 AI Generator 最根本的区别。

------------------------------------------------------------------------

# 18. 当前阶段对 Digger v1 的研究性判断

以下不是最终产品决策，而是三轮研究后值得进入下一阶段验证的假设。

## 18.1 Digger 不应是万能 AI Workspace

它应该有明确领域边界：

> 以文字、图片、音频、视频和网页内容为主要 Output 的 Creator Project。

## 18.2 不应从 Editor 出发定义产品

Editor 很重要，但它只是 Project 的一个 View / Interaction Surface。

Digger 更深层的核心是 Project 本身。

## 18.3 不应把 Chat 作为产品中心

Chat 是 Human-Agent Interaction 的一种方式。

在很多场景中，更重要的交互可能是：

-   Selection → Agent
-   Object → Action
-   Candidate → Diff
-   Timeline → Edit
-   Source → Claim
-   Analytics → Learning Candidate
-   Project Truth → Constraint

## 18.4 不应强行统一三个领域

Social、Writing、Site 应拥有各自专业的 Project Experience。

共享部分应尽量停留在真正稳定的基础层。

## 18.5 Project Truth 值得重点验证

它可能成为 Digger 区别于普通 Memory/RAG Creator 的关键设计概念。

## 18.6 Candidate / Official State 分离值得进入核心模型

这是 Agent 参与长期 Project 后非常稳定的治理需求。

## 18.7 Publication 应成为一等对象

尤其在 Social 与 Website 中，Production 和 Publication 是两个不同阶段。

## 18.8 复杂 Media 可能需要 Subproject

视频是最明显的例子。它自身已经拥有完整生产生命周期。

## 18.9 Feedback 不应止于 Analytics

Agent-native Creator 应探索：

``` text
Feedback → Learning Candidate → Project Evolution
```

------------------------------------------------------------------------

# 19. 尚未解决的问题

当前调研并没有回答以下问题，因此不应在 v1 设计中未经验证直接定案。

### 19.1 Project 的共同最小模型是什么？

是否真的需要统一的：

-   Knowledge
-   Structure
-   Rule
-   Candidate
-   Publication
-   Feedback

还是其中部分应该完全留在领域层？

### 19.2 Project Truth 是否应该是实体？

可能的实现包括：

-   独立对象；
-   Metadata / Authority Layer；
-   Knowledge Graph 中的状态；
-   Policy；
-   多种领域对象的统称。

目前不宜过早数据建模。

### 19.3 Candidate 应该统一到什么程度？

文本 Diff、图片修改、视频 Timeline Change、Schema Change
的审阅方式完全不同。

共同的可能只是：

``` text
Proposal → Decision → Official State
```

而不是统一的数据结构或 UI。

### 19.4 Agent、Skill、Tool、Workflow 的边界

OpenMontage 等项目表明这四层具有不同职责，但 Digger
应如何抽象仍需进一步研究。

### 19.5 Project 与 Subproject 的关系

尤其是：

``` text
Social Project
  └── Video Project
```

以及：

``` text
Site Project
  └── Research / Article Project
```

需要研究其 Context、Asset、Truth 和 Version 如何继承。

### 19.6 Human / Agent 权限模型

不同 Agent 是否具有不同：

-   Read Scope
-   Write Scope
-   Tool Scope
-   Approval Level
-   Publication Permission

这可能成为 Agent-native Creator 的重要基础设施。

### 19.7 Creator 的本地优先与云端边界

大型素材、视频、私有研究资料、版权素材、网站发布等会产生完全不同的存储与执行需求，需要在架构阶段单独研究。

------------------------------------------------------------------------

# 20. 下一阶段建议：从竞品研究进入领域建模

当前横向产品样本已经覆盖：

### AI-native 完整样本

-   Easel
-   WordBird
-   NarraLume
-   SiteSpec
-   Caelo CMS

### 领域补全样本

-   Postiz
-   OpenMontage
-   Synthetix
-   OpenSEO
-   Payload

### 成熟控制组

-   Mixpost
-   Socioboard
-   Scrivener
-   novelWriter
-   bibisco
-   Payload
-   Directus

继续无目的增加样本的边际收益已经下降。

下一阶段建议转入：

> **Digger Creator Domain Modeling**

建议顺序：

``` text
Research Findings
      ↓
Domain Objects
      ↓
Domain Boundaries
      ↓
Cross-domain Common Model
      ↓
Project Lifecycle
      ↓
Human / Agent / Tool Model
      ↓
Knowledge / Truth Model
      ↓
Candidate / Change Governance
      ↓
Publication / Feedback Model
      ↓
Digger v1 Product Scope
```

此阶段仍应先于 UI、技术选型和详细 PRD。

------------------------------------------------------------------------

# 21. 调研对象索引

## 21.1 Social Creator

-   **Easel** --- AI-native Creator workspace\
    https://github.com/ZJU-REAL/Easel

-   **fengyun-publish** --- 微信深度内容生产链\
    https://github.com/duliangkuan/fengyun-publish

-   **wechat-publisher** --- Skill-based 微信发布流程\
    https://github.com/jiji262/wechat-publisher

-   **xiaohongshu-agent** --- 小红书内容研究与生产\
    https://github.com/jailbreak07/xiaohongshu-agent

-   **ContentGen_AI** --- 多 Agent 内容生产\
    https://github.com/RevanthAI/ContentGen_AI

-   **LangChain social-media-agent** --- Human-in-the-loop Social Agent\
    https://github.com/langchain-ai/social-media-agent

-   **Postiz** --- Social Operations / Distribution\
    https://github.com/gitroomhq/postiz-app

-   **Mixpost** --- 成熟 Social Management 控制组\
    https://github.com/inovector/mixpost

-   **Socioboard** --- Social CRM / Operations 控制组\
    https://github.com/socioboard/Socioboard-5.0

## 21.2 Video / Media Production

-   **OpenMontage** --- Agentic Video Production\
    https://github.com/tjebastin/openmontage

-   **OpenNolan** --- 短视频自动生产\
    https://github.com/het8802/OpenNolan

-   **MoneyPrinterTurbo** --- Topic-to-Video Pipeline\
    https://github.com/harry0703/MoneyPrinterTurbo

-   **SupoClip** --- 长视频理解与短视频重构\
    https://github.com/SupoAI/supoclip

-   **auto-editor** --- 自动剪辑基础设施\
    https://github.com/WyattBlue/auto-editor

-   **OpenCut** --- Human Video Editor\
    https://github.com/OpenCut-app/OpenCut

-   **ComfyUI** --- Visual Workflow Engine\
    https://github.com/comfyanonymous/ComfyUI

-   **Remotion** --- Programmatic Video\
    https://github.com/remotion-dev/remotion

## 21.3 Long-form

-   **WordBird** --- AI-assisted Long-form Writing Workspace\
    https://github.com/thimbleberrysystems/WordBird

-   **NarraLume** --- Agent-native Long-form Writing\
    https://github.com/abligail/narralume

-   **Story Studio Cloud** --- Fiction Engineering\
    https://github.com/xiangyucao/story-studio-cloud

-   **xnovelist** --- Offline Novel Writing\
    https://github.com/giapnguyen74/xnovelist

-   **Synthetix** --- Research-driven Long-form Writing\
    https://github.com/WalkCloud/Synthetix

-   **novelWriter** --- 成熟开源长篇写作控制组\
    https://github.com/vkbo/novelWriter

-   **bibisco** --- Novel Project 控制组\
    https://github.com/andreafeccomandi/bibisco

-   **Scrivener** --- 专业长篇写作产品控制组\
    https://www.literatureandlatte.com/scrivener/overview

## 21.4 AGC / OGC Website

-   **Caelo CMS** --- Agent-native CMS\
    https://github.com/caelo-cms/caelo-cms

-   **Headcode CMS** --- Human + Agent Structured CMS\
    https://github.com/headcodecms/headcodecms

-   **SiteSpec** --- Git-native Content System\
    https://github.com/sitespec/sitespec

-   **OpenSEO** --- AI SEO / Programmatic Content\
    https://github.com/Juliusolsson05/openSEO

-   **LightCMS** --- Content Fork / Diff / Merge / Review\
    https://github.com/jonradoff/lightcms

-   **Payload** --- Mature Headless CMS / Application Framework\
    https://github.com/payloadcms/payload

-   **Directus** --- Mature Data / Content Platform\
    https://github.com/directus/directus

-   **Strapi** --- Mature Headless CMS\
    https://github.com/strapi/strapi

-   **Astro** --- Content-oriented Web Framework\
    https://github.com/withastro/astro

-   **Docusaurus** --- Structured Knowledge Publishing\
    https://github.com/facebook/docusaurus

------------------------------------------------------------------------

# 22. 结论

三轮调研之后，Digger 的研究对象已经从模糊的"AI Creator"逐渐清晰为：

> **面向长期 Creator Project 的 Human-Agent 协同生产与演化系统。**

其关键价值并不在于单次生成能力。

单次生成能力最终会高度商品化：

``` text
Generate Text
Generate Image
Generate Video
Generate Page
```

真正更难、也更有长期价值的问题是：

``` text
如何理解一个长期 Project？
如何维护它的 Knowledge？
如何识别并遵守它的 Truth？
如何在已有 Structure 中工作？
如何让 Agent 的修改可治理？
如何把 Production 与 Publication 分离？
如何让 Feedback 成为下一轮生产能力？
如何让 Project 随时间积累而不是重新开始？
```

因此 Digger 当前最值得保留的研究命题是：

> **Output 会消耗，Project 会积累。**

如果这一命题成立，那么 Digger 的核心就不应该是"更强的生成器"，而应该是：

> **让 Human、Agent 与 Tool 围绕一个长期存在的 Creator Project
> 持续工作，并让这个 Project 本身越来越有能力创造。**

这构成后续 Digger v1 领域建模与产品定义的研究基础。
