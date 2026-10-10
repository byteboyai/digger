# ByteBoy Digger GitHub 项目代码功能分析 v0.1

> 分析对象：`ByteBoy_Digger_v1_调研分析报告.md` 引用的 GitHub 仓库  
> 分析口径：以本地浅克隆中的代码、路由、数据模型、测试和适配器为准  
> 分析时间：2026-09-30  
> 状态：Working Draft

---

## 1. 分析说明

本报告回答的是“仓库中的代码实际具备什么功能”，不是复述产品宣传。

判断证据优先级：

1. 业务实现、路由、服务、数据模型；
2. 自动化测试和示例；
3. 可执行脚本与配置；
4. README 仅用于解释代码意图。

成熟度标签：

- **完整产品**：具有较完整 UI、后端、数据模型和业务闭环；
- **专项产品**：聚焦单一创作或发布链路，功能可运行；
- **能力框架**：提供可组合基础能力，不直接构成完整创作产品；
- **早期实现**：代码已存在，但覆盖范围或产品闭环有限。

本次共分析 33 个成功克隆的仓库。`SupoAI/supoclip` 返回 Repository not found，无法进行代码分析。

---

## 2. Social Creator / 内容发布

### 2.1 Easel

**定位与成熟度：专项产品；Agent-first 多模态内容工作台。**

代码已证实的功能：

- 使用账号画像目录保存受众、身份、平台、风格、偏好和记忆；
- 以 Skills 组织论文解读、小说、短剧、电商图、信息图、公众号排版等内容生产流程；
- 提供图片生成、图片编辑、卡片与信息图模板；
- 提供 TTS、字幕、音视频处理和视频生产流水线；
- 提供跨平台差异化改写、发布调度、发布分析和质量门禁；
- 提供终端会话和 Web 工作台；
- 支持 OpenAI/Gemini 等模型适配器以及 OpenClaw Skill 运行方式。

主要证据：`profiles/_template/*`、`skills/openclaw/*`、`skill-cross-platform-publish/scripts/publish_dispatch.py`、`skill-publish-scheduler/scripts/publish_queue.py`、`skill-publish-analytics/scripts/analyze.py`、`web/frontend`。

边界：大量能力由 Skill 指令、模板和脚本组合而成，并非全部都有统一的强类型领域状态或事务式工作流。

### 2.2 fengyun-publish

**定位与成熟度：专项产品；公众号文章选题、写作、审核与发布流水线。**

代码已证实的功能：

- 从 RSSHub、微信 Feed、TrendRadar 等来源采集热点和候选主题；
- 建立语料索引、清洗语料并选择 few-shot 参考；
- 由 Writer、封面、内容评审、话术评审等多个 Agent 角色协作；
- 对标题、开头、结尾、事件和措辞做重复检测；
- 三轨 Critic 投票与 Gate 门禁，阻止不合格内容进入发布阶段；
- 公众号 HTML 布局规则、配图决策、图片生成和图片缓存；
- 发布到微信公众号，并记录运行日志、调用日志和发布指标；
- 具备飞轮报告、指标导入和稳定性测试；
- 附带实验性视频模块和剪映模板输出。

主要证据：`.claude/agents/*`、`tools/topic_recommender.py`、`tools/critic_vote.py`、`tools/gate.py`、`tools/post_fengyun_publish.py`、`tools/flywheel_report.py`、`assets/run_log.schema.json`、`video_module/*`。

边界：更接近 Claude Code 驱动的工程化 SOP；部分运行依赖外部容器、Cookie、API 密钥和本地未入库数据。

### 2.3 wechat-publisher

**定位与成熟度：专项工具；Markdown 到微信公众号草稿箱。**

代码已证实的功能：

- Markdown 转微信公众号兼容 HTML；
- 15 组以上文章主题和主题预览；
- 本地图片处理、公众号素材上传及 URL 替换；
- 调用微信 API 获取 Token、上传封面并创建草稿；
- AI 配图生成，包含多套图片风格预设；
- AI 文风痕迹评分与中文 humanizer 词表检查；
- newspic/卡片化排版模式；
- 多平台发布入口和独立 CLI/Skill 使用方式；
- 配置解析、HTML 转换和评分逻辑具有测试。

主要证据：`scripts/html_converter.py`、`scripts/image_handler.py`、`scripts/wechat_api.py`、`scripts/publish.py`、`scripts/generate_image.py`、`scripts/ai_score.py`、`assets/themes/*`、`tests/*`。

边界：核心是公众号草稿发布，不是社媒内容日历、账号运营或分析平台。

### 2.4 xiaohongshu-agent

**定位与成熟度：早期实现；浏览器驱动的小红书研究 Agent。**

代码已证实的功能：

- 通过 Playwright 复用登录态访问小红书；
- 搜索、读取页面和提取内容；
- LLM 工具循环，可启动不同分析视角的子 Agent；
- 将分析结果写入 Markdown 输出；
- 保存 LLM 请求/响应、工具调用和会话日志；
- 支持 OpenRouter 等模型配置。

主要证据：`src/*`、`prompts/*`、`config/*`、`docs/XIAOHONGSHU_AUTH.md`、仓库中的实际 `logs/tool-*` 与 `outputs/*`。

边界：仓库包含大量浏览器 Profile/缓存数据；重点是研究与分析，不是稳定的发布、账号管理或指标闭环。

### 2.5 ContentGen_AI

**定位与成熟度：早期实现；小型多 Agent 内容生成服务。**

代码已证实的功能：

- 以主入口启动内容生成 Agent；
- 将研究、写作等职责拆成 Agent/任务；
- 接收主题并生成内容结果；
- 提供容器化启动配置。

主要证据：`main.py`、`content_generation_agent/*`、`requirements.txt`、`DockerFile`。

边界：仓库仅约 18 个文件，没有完整的项目状态、发布渠道、版本管理或反馈分析。

### 2.6 langchain-ai/social-media-agent

**定位与成熟度：专项产品；基于 LangGraph 的社媒内容策划和发布 Agent。**

代码已证实的功能：

- 从 Twitter/X、Reddit、GitHub Trending、AI Newsletter、Slack 和网页采集素材；
- 去重、分组、重新分组和验证候选内容；
- 生成单帖、Thread、内容报告和 campaign plan；
- 将 URL/Slack 等已有内容改造成多条社媒内容；
- 查找、生成、重排并验证配图；
- 设置 Human-in-the-loop interrupt，允许审核和重写；
- 安排发布时间并上传到 Twitter/X、LinkedIn 等渠道；
- 保存已用链接、帖子来源和反思记录；
- 提供图执行测试、集成测试和 LLM eval。

主要证据：`src/agents/curate-data/*`、`generate-post/*`、`generate-thread/*`、`repurposer/*`、`find-and-generate-images/*`、`shared/nodes/schedule-post.ts`、`clients/*`、`evals/*`。

边界：这是 Agent 工作流参考实现，不具备 Postiz 一类完整的多租户社媒管理后台。

### 2.7 Postiz

**定位与成熟度：完整产品；多平台社媒运营平台。**

代码已证实的功能：

- 多组织/用户认证、权限、订阅和管理后台；
- 连接大量社媒渠道，代码中包含 X、LinkedIn、Facebook、Instagram、YouTube、TikTok、Reddit、Discord、Telegram、Mastodon、Bluesky、Pinterest、WordPress 等平台资源与适配逻辑；
- 创建、验证、预览、排期、发布和自动发布帖子；
- 媒体上传、媒体库和外部上传组件；
- 内容日历、帖子集合、通知和签名；
- 渠道分析与聚合 Analytics；
- OAuth 应用、Webhook 和公开 API；
- AI Copilot、Agent 命令任务和第三方插件入口；
- 浏览器扩展用于补充 Cookie 型集成；
- Token 刷新、后台命令和异步编排。

主要证据：`apps/backend/src/api/routes/*`、`apps/backend/src/integrations/*`、`apps/frontend/src/app/(app)/(site)/*`、`apps/commands/src/tasks/*`、`apps/extension/*`、数据库 schema/migrations。

边界：部分平台或高级能力可能受 SaaS/订阅策略限制；平台图标存在不等于每个适配器能力完全对等。

### 2.8 Mixpost

**定位与成熟度：完整产品；Laravel 社媒排期与发布组件。**

代码已证实的功能：

- 社媒账号 OAuth 连接和账号实体管理；
- 帖子创建、复制、删除、草稿和排期；
- 定时任务扫描并发布到平台；
- 内容日历和 Dashboard；
- 图片/视频上传、媒体库、媒体转换；
- Unsplash、Tenor 等素材服务；
- Facebook、X/Twitter、Mastodon 等平台预览与 Provider 抽象；
- 队列 Job、速率限制、失败处理和响应状态建模；
- Provider 报告/指标接口。

主要证据：`src/Http/Controllers/*`、`src/Jobs/*`、`src/SocialProviders/*`、`src/Commands/RunScheduledPosts.php`、`resources/js/Components/Calendar/*`、`Media/*`、`ProviderGallery/*`。

边界：开源仓库是可嵌入 Laravel 的核心包；平台数量和分析深度少于大型全栈产品。

### 2.9 Socioboard 5.0

**定位与成熟度：完整但遗留负担较重的社媒管理套件。**

代码已证实的功能：

- 用户、团队、成员、邀请、Board 和权限管理；
- 社媒账号授权与 Profile 管理；
- 跨平台发布、草稿、排期、审批和已发布记录；
- Feed 聚合、消息/聊天和通知；
- Twitter、Instagram、LinkedIn、YouTube、Pinterest、Tumblr 等平台数据模型；
- 趋势、Hashtag、RSS、新闻、Pixabay、Giphy、Imgur 等发现与素材来源；
- 团队报表、平台 Insights 和邮件报告；
- Node 微服务式 API 与 PHP Web 前端，提供 Docker/Ansible 部署。

主要证据：`socioboard-api/*`、`Common/Models/*`、`Common/Mongoose/models/*`、`socioboard-web-php/Modules/*`。

边界：存在重复命名文件和大小写冲突，模块间风格不统一；平台 API 的时代差异可能导致部分适配代码需要更新。

---

## 3. Media Production / 视频与生成媒体

### 3.1 OpenMontage

**定位与成熟度：专项产品；Agent 驱动的端到端视频生产系统。**

代码已证实的功能：

- 从参考视频分析、脚本、镜头计划到最终渲染的流水线；
- Living Storyboard/Backlot 工作台追踪镜头和制作状态；
- AI 图像、AI 视频、库存素材、音乐、音效、TTS 和 Avatar 多 Provider；
- FFmpeg 合成、字幕、转场、音频反应和多轨处理；
- Remotion、Manim、HTML/GSAP/Hyperframes 等多种画面生成方式；
- 风格系统、平台输出 Profile 和大量运动图形预设；
- Provider 评分选择、质量门禁、预算控制和决策审计；
- Agent Skills、CLI 与契约测试。

主要证据：`pipelines/*`、`tools/*`、`backlot/*`、`.agents/skills/*`、`remotion-composer/*`、`tests/*`。

边界：许多生成路径依赖外部模型/API；仓库同时包含大量 Skill 知识资产，需区分编排说明与真正执行代码。

### 3.2 OpenNolan

**定位与成熟度：专项产品；OpenMontage 系谱的本地 AI 视频编辑器。**

代码已证实的功能：

- 桌面端与 Web 端视频项目操作；
- 时间线/编辑引擎、镜头组合和渲染；
- 字幕、语音、音乐与音频处理；
- Remotion、Manim、Hyperframes 等生成与编辑能力；
- AI 视频、Avatar、素材和语音 Provider 接入；
- 输出比例/Profile、视觉风格、质量检查和 Prompt Gallery；
- 零 API Key 的本地/免费素材路径，以及可选云模型路径。

主要证据：`desktop/*`、`web/*`、`composition/*`、`tools/*`、`.agents/app/skills/*`、`tests/*`。

边界：与 OpenMontage 有显著共享能力和代码谱系，不应在竞品统计中当作完全独立的两套架构样本。

### 3.3 MoneyPrinterTurbo

**定位与成熟度：专项产品；一键生成短视频。**

代码已证实的功能：

- 根据主题生成文案；
- 从素材源检索视频/图片或使用本地素材；
- 多 TTS Provider 配音；
- 自动字幕生成、切分、样式和定位；
- BGM 选择与音量混合；
- FFmpeg/MoviePy 合成竖屏或横屏成片；
- 批量生成多个视频；
- Web UI、CLI、REST 服务与 Docker/GPU 部署；
- 配置多种 LLM、语音、素材和代理参数。

主要证据：`app/services/*`、`app/controllers/*`、`app/models/*`、`app/utils/video_utils.py`、`main.py`、`cli.py`、`test/*`。

边界：它是模板化自动成片流水线，不是精细非线性编辑器或长期媒体项目管理系统。

### 3.4 auto-editor

**定位与成熟度：专项工具；按媒体信号自动裁剪视频/音频。**

代码已证实的功能：

- 分析音量、静音、运动和字幕等信号；
- 根据表达式决定保留、加速或删除时间段；
- 智能渲染音频、视频和字幕；
- 支持 H.264、HEVC 等编码路径；
- 输出成片或编辑时间线；
- 导出 Final Cut Pro 7/11、Shotcut、Kdenlive、MLT、OTIO、JSON；
- 提供字幕转录和编辑效果相关 Skills。

主要证据：`src/analyze/*`、`src/timeline.nim`、`src/render/*`、`src/exports/*`、`skills/*`。

边界：不生成内容，核心价值是确定性媒体分析与时间线变换。

### 3.5 OpenCut

**定位与成熟度：早期实现；开源跨平台视频编辑器。**

代码已证实的功能：

- Web 编辑器路由和桌面应用外壳；
- Rust 媒体处理 crate；
- 桌面时间线 Panel；
- FFmpeg 跨平台构建基础设施；
- API、Web、Desktop monorepo 结构。

主要证据：`apps/web/src/routes/editor.tsx`、`apps/desktop/src/panels/timeline.rs`、`crates/*`、`.github/workflows/media-deps.yml`。

边界：当前克隆版本只有约 130 个文件，README 也明确标记开发状态；尚不能视为具备成熟剪辑工作流的完整产品。

### 3.6 ComfyUI

**定位与成熟度：能力框架；节点式生成媒体执行引擎。**

代码已证实的功能：

- 图形化节点工作流、依赖图执行、队列和缓存；
- 文生图、图生图、局部重绘、扩图、ControlNet、深度/姿态/分割等图像能力；
- 文生视频、图生视频、首尾帧视频、视频合并；
- 音频生成以及 3D/高斯泼溅等扩展工作流；
- 模型加载、显存管理、量化和多种硬件后端；
- 自定义节点、节点替换和模型管理；
- HTTP/WebSocket API、历史记录、上传与资产管理；
- 工作流 JSON 蓝图及大量示例。

主要证据：`execution.py`、`comfy_execution/*`、`nodes.py`、`comfy/*`、`api_server/*`、`app/custom_node_manager.py`、`blueprints/*`。

边界：它提供生成执行图，不负责 Digger 所需的创作项目语义、治理、出版或反馈闭环。

### 3.7 Remotion

**定位与成熟度：能力框架；以 React/TypeScript 编程生成视频。**

代码已证实的功能：

- 使用 React 组件、帧和时间序列定义视频；
- Studio 实时预览、时间线和参数编辑；
- CLI 渲染视频、静帧和图片序列；
- 浏览器、本地、Lambda 与 Cloud Run 渲染；
- 音频、视频、图片、字体、动画和转场组件；
- SRT 解析/序列化、TikTok 风格字幕和行长处理；
- Canvas/DOM/Web Renderer 等渲染路径；
- Player 嵌入、Bundler、媒体解析、WebCodecs/FFmpeg 相关能力；
- 并发渲染、队列、进度和编解码器配置。

主要证据：`packages/core`、`packages/studio`、`packages/renderer`、`packages/cli`、`packages/player`、`packages/captions`、`packages/lambda`、`packages/cloudrun`。

边界：它是视频渲染基础设施，不负责内容策划、素材决策或项目治理。

---

## 4. Long-form Writing / 长篇写作

### 4.1 WordBird

**定位与成熟度：完整产品；本地优先的 Agentic 小说写作桌面应用。**

代码已证实的功能：

- 项目向导、章节树、Outline 和多标签 Markdown 编辑器；
- Story Bible 与 Continuity 检查；
- 稿件导入、整书编译和主题化导出；
- Revision 引擎和可恢复的编辑缓冲；
- Agent 读取项目上下文、研究记录和文件；
- Agent 修改以 Diff/Plan 卡片呈现，支持逐项或多文件审核后应用；
- 项目范围隔离、健康检查和 Research Ledger；
- Electron 桌面端、MCP/Agent SDK 桥接；
- 大量端到端测试覆盖导入、导出、审核和编辑。

主要证据：`services/novel/*`、`services/ai/*`、`components/agent/AgentDiffView.vue`、`agentMultiFileApply.ts`、`RevisionService.ts`、`test/e2e/*`。

边界：核心存储以本地文件项目为中心，协作和云端多用户不是主要方向。

### 4.2 NarraLume

**定位与成熟度：完整产品；长篇写作、Canon 与 AI 修订平台。**

代码已证实的功能：

- 书架、项目、文档、章节、Outline 和写作台；
- Story Bible/知识编辑器与 Canon Fact 生命周期；
- 从稿件中提取 Canon Candidate，由用户接受、拒绝或撤回；
- 跨章节上下文、设定 grounding 和一致性检查；
- AI 快速创作、章节 Worker、修订请求和修订应用服务；
- OpenAI Chat/Responses、Anthropic、Embeddings 和结构化输出网关；
- 导入、导出、备份、恢复和项目统计；
- 持久化迁移覆盖协作版本、LLM 中断、写保护和跨章节结算；
- Web、Server、Bridge、Relay 分层以及较完整测试。

主要证据：`packages/domain/*`、`packages/narrative/*`、`packages/context/*`、`packages/persistence/*`、`packages/llm/*`、`apps/web/src/workspaces/*`。

边界：功能广且仍快速演进；需特别关注 migration 与领域状态兼容，而不是只复用 UI。

### 4.3 Story Studio Cloud

**定位与成熟度：专项产品；云端故事规划与外部 Agent 协作。**

代码已证实的功能：

- 登录、创建作品和管理章节；
- 作品 Outline、角色、地点、时间线/故事记忆数据；
- 章节编辑、连续性审核和可恢复章节历史；
- 导入/导出便携 Story Memory JSON；
- MCP 接口供 Codex 等外部 Agent 读取和更新项目；
- 完成作品的公开分享；
- Drizzle 数据模型和 API 路由。

主要证据：`app/api/works/*`、`app/api/import/route.ts`、`app/api/mcp/*`、`db/*`、`drizzle/*`、`lib/*`、`tests/*`。

边界：项目明确不内置 AI 写作，AI 通过用户自己的外部 Agent/MCP 接入。

### 4.4 xnovelist

**定位与成熟度：专项产品；AI 辅助小说策划与写作 Web 应用。**

代码已证实的功能：

- 小说项目、人物、世界观、情节和章节工作区；
- Beat/场景级写作资产；
- AI 头脑风暴、Outline 生成和质量检查；
- 编辑面板与 Outline 联动；
- 文档上传、转换、分块、Embedding 和知识模式；
- 文档结构识别和层级拆分；
- 自动标签、知识图式 Chunk 和检索；
- 稿件导出。

主要证据：`src/components/writing/*`、`src/hooks/brainstorm/*`、`src/lib/brainstorm/*`、`src/lib/documents/*`、`src/hooks/writing/use-export.ts`。

边界：更偏 AI 生成与知识处理；长期 Canon 治理、审批式修改和出版版本链不如 WordBird/NarraLume 清晰。

### 4.5 Synthetix

**定位与成熟度：完整产品；本地知识库与长文生成桌面应用。**

代码已证实的功能：

- 上传和管理 PDF/文本等资料；
- 文档转换、分块、Embedding、知识图和向量检索；
- 文档处理队列、状态、重试和孤儿清理；
- 头脑风暴、摘要与多级 Outline；
- 基于知识库生成长文并在写作编辑器中编辑；
- 多模型/Provider 配置；
- Electron/Next.js 桌面应用与 Python Worker；
- 导出生成内容。

主要证据：`src/lib/documents/*`、`src/lib/brainstorm/*`、`src/components/documents/*`、`src/components/writing/*`、`workers/python/*`、`electron/*`。

边界：更像 RAG 驱动的研究写作工具，不以小说 Canon、角色或出版 Edition 为核心。

### 4.6 novelWriter

**定位与成熟度：完整产品；非 AI、本地优先的小说写作桌面软件。**

代码已证实的功能：

- 项目树组织 Part、Chapter、Scene、人物、地点和对象；
- 专用纯文本/Markdown 风格编辑器、语法高亮、搜索和历史；
- Outline 视图和项目全局搜索；
- 标签、状态、交叉引用与统计；
- 拆分、合并、复制项目文档；
- Manuscript Build，输出 DOCX、ODT/FODT、EPUB、HTML、Markdown 等；
- 项目 XML 格式版本迁移与恢复；
- 多语言桌面 UI 和大量回归测试。

主要证据：`novelwriter/core/*`、`editor/*`、`gui/outline.py`、`manuscript/*`、`tools/*`、`tests/*`。

边界：没有内置 LLM/Agent；价值主要在成熟的写作领域模型、项目文件格式和编译导出。

### 4.7 bibisco

**定位与成熟度：完整产品；结构化小说设计与分析桌面软件。**

代码已证实的功能：

- 章节、场景、Part 和拖拽排序；
- 主次人物建模、人物访谈问卷、事件、图片和人物关系；
- 地点、物品、组织、叙事线和标签；
- 富文本写作、字数/字符统计和修订选择；
- 章节长度、人物出场和分布分析；
- Project Explorer 汇总小说元素；
- 导出 PDF、DOCX、TXT、EPUB 等格式；
- 本地桌面项目管理。

主要证据：`bibisco/app/components/chapters/*`、`characters/*`、`analysis/*`、`project-explorer/*`、`export/*`。

边界：开源 Community Edition 与付费版功能需要区分；仓库中的核心代码不等于官网列出的全部高级功能。

---

## 5. AGC/OGC Website / CMS 与站点生产

### 5.1 Caelo CMS

**定位与成熟度：完整产品；AI-native、可视化编辑与静态部署 CMS。**

代码已证实的功能：

- Page、Template、Module、Placement 和 Content Instance 组合式内容模型；
- 内容库、媒体库、页面编辑、SEO 和 Preview；
- 版本历史、快照与回看；
- 主题、设计草稿和字体管理；
- Staging Preview、Build/Deploy Run 和静态生成；
- 内容聊天与流式 Agent 工具卡；
- Plugin SDK、Plugin Host、沙箱与组件 Kit；
- MCP Server、API Gateway 和 Edge Router；
- 角色权限，代码测试包含 reviewer readonly 场景；
- 媒体上传与异步状态查询。

主要证据：`apps/admin/src/routes/*`、`apps/static-generator/*`、`packages/plugin-*`、`packages/mcp-server/*`、`ARCHITECTURE.md`、`e2e-livedit/*`。

边界：快速演进的新项目；插件安全、升级兼容和发布事务仍应通过实际部署验证。

### 5.2 Headcode CMS

**定位与成熟度：专项产品；Agent-first、代码内嵌式小型 CMS。**

代码已证实的功能：

- Entry、Page、富文本和图片字段；
- Convex 数据模型与版本记录；
- Draft/Live 分离；
- 管理后台登录、条目、图片和版本页面；
- Next.js 站点直接消费 CMS 内容；
- MCP 路由供 Agent 操作内容；
- `llms.txt` 与 Headcode Markdown 输出，方便机器读取站点；
- 可配置字段定义。

主要证据：`convex/schema.ts`、`convex/db/versions.ts`、`headcode/fields.ts`、`headcode/versions.ts`、`app/(admin)/*`、`app/(mcp)/[transport]/route.ts`、`app/(site)/*`。

边界：约 168 个文件，属于轻量实现；不具备大型 CMS 的复杂关系、工作流、市场化插件和多渠道能力。

### 5.3 SiteSpec

**定位与成熟度：能力框架；文件化网站规范、验证器与静态构建工具。**

代码已证实的功能：

- 用 YAML/JSON 描述 Site、Page、Component、Collection、Design System 和 UI Field；
- JSON Schema 验证规范和内容；
- Typed Collection、关系查询、分页和导航；
- 媒体与 SEO 元数据；
- Inspector 支持选择、命名和检查页面 Block；
- Astro 适配与模板项目；
- CLI 创建、验证、构建和部署 GitHub Pages；
- 构建状态与规范版本处理。

主要证据：`packages/core/src/schemas/*`、`content-query.ts`、`routes.ts`、`packages/cli/src/*`、`packages/astro/*`、`tests/*`。

边界：无在线数据库和传统 CMS Runtime；编辑单位是 Git 中的结构化文件。

### 5.4 openSEO

**定位与成熟度：专项产品；SEO 内容研究、生成、发布与监测平台。**

代码已证实的功能：

- 多租户项目与站点配置；
- 关键词、竞争页面和搜索结果研究；
- Content Brief、Outline、文章生成和编辑；
- 文档/站点内容摄取与语义检索；
- AI Agent/Skill 驱动的博客生产；
- 发布和同步适配器；
- SEO 审计、排名/性能数据与分析界面；
- 后台任务、队列和定时执行；
- Next.js API/UI 与数据库持久化。

主要证据：`src/app/api/*`、`src/lib/*`、`agents/*`、`ARCHITECTURE.md`、数据库 schema/migrations、测试目录。

边界：它优化的是 SEO 内容运营，不是通用网站视觉搭建器或完整部署平台。

### 5.5 LightCMS

**定位与成熟度：专项产品；Go 实现的轻量 Headless CMS 与 Analytics 服务。**

代码已证实的功能：

- 内容条目和集合的 CRUD/API；
- 用户、认证和权限；
- 编辑历史/版本相关查询；
- 媒体与站点配置；
- 页面访问和内容表现 Analytics；
- MCP Server 入口，允许 Agent 访问 CMS；
- Webhook/集成能力；
- 单二进制/容器部署配置。

主要证据：`cmd/*`、`internal/*`、`routes/*`、`MCP.md`、`config/*`、`go.mod`。

边界：与 Directus/Payload/Strapi 相比领域面较小，适合学习精简 API 和分析闭环，而不是直接作为复杂内容平台基线。

### 5.6 Payload CMS

**定位与成熟度：完整产品；代码优先的 TypeScript Headless CMS。**

代码已证实的功能：

- 使用 TypeScript 配置 Collection、Global、Field、Validation 和 Hook；
- Local API、REST、GraphQL；
- PostgreSQL、MongoDB 等数据库适配器；
- Auth、RBAC、字段级访问控制和 API Key；
- Draft、Autosave、Versions、Preview 和发布状态；
- Upload、图片尺寸处理和多种对象存储适配器；
- Relationship、Join、Blocks、Array、Localization；
- Job Queue、Schedule Publish 和后台任务；
- 自定义 Admin UI、组件和插件；
- Live Preview、Form Builder、SEO、Search、Redirect 等插件；
- Next.js 深度集成和大量示例。

主要证据：`packages/payload/*`、`packages/ui/*`、`packages/db-*`、`packages/storage-*`、`packages/plugin-*`、`examples/*`、`test/*`。

边界：是 CMS 基础平台，不直接提供 Digger 的 Project/Intent/ProductionRun/Change Governance 语义。

### 5.7 Directus

**定位与成熟度：完整产品；数据库之上的数据平台与 Headless CMS。**

代码已证实的功能：

- 对现有 SQL 数据库进行动态 Collection/Field/Relation 建模；
- REST、GraphQL、WebSocket/Realtime API；
- Data Studio 管理内容、文件和关系；
- 用户、角色、Policy、字段/项目级权限；
- Revision、Activity、Comment 和 Accountability；
- Flow 自动化、Trigger、Operation、Webhook 和 Schedule；
- Files、Folders、图片变换和对象存储；
- Dashboard、Panel 和数据洞察；
- Extension SDK 支持 Interface、Display、Layout、Module、Endpoint、Hook、Operation；
- Content Version、Translation 和 Sharing。

主要证据：`api/src/services/*`、`api/src/controllers/*`、`app/src/modules/*`、`packages/extensions-sdk/*`、`packages/schema/*`、数据库 migrations。

边界：模型是通用数据平台，Creator Domain 语义需要 Digger 自己构建，不能直接将 Collection 当成统一 Artifact。

### 5.8 Strapi

**定位与成熟度：完整产品；插件化 Node.js Headless CMS。**

代码已证实的功能：

- Content-Type Builder 定义 Collection Type、Single Type、Component、Dynamic Zone 和 Relation；
- Content Manager 创建、编辑、过滤和批量处理文档；
- Draft & Publish、Document Service 和内容历史；
- REST、GraphQL 和服务端扩展 API；
- 用户、Admin RBAC、API Token 和权限检查；
- Upload/Media Library 和 Provider；
- i18n、本地化内容；
- Preview、Content Release 和 Review Workflow；
- Plugin 系统、Marketplace 和 Admin 扩展；
- Lifecycle、Cron、Event Hub 和 Webhook。

主要证据：`packages/core/*`、`packages/plugins/*`、`packages/cli/*`、`docs/docs/docs/01-core/*`、`tests/*`。

边界：部分 Review Workflow、Release 等能力可能属于 Enterprise 分层；报告使用时应分别标注 OSS 与商业功能。

### 5.9 Astro

**定位与成熟度：能力框架；内容型网站框架和多模式渲染 Runtime。**

代码已证实的功能：

- 文件系统路由、动态路由、API Endpoint 和 Middleware；
- 静态生成、SSR、混合/按需渲染；
- Content Collections、Schema 校验、Markdown/MDX；
- Islands 架构和 React/Vue/Svelte/Solid/Preact 等集成；
- 图片优化、字体、国际化路由和 Actions；
- Adapter 部署到 Node、Cloudflare、Vercel、Netlify 等环境；
- Dev Server、构建器、CLI 和升级工具；
- RSS/Sitemap 等内容站常用集成。

主要证据：`packages/astro/*`、`packages/content/*`、`packages/integrations/*`、`packages/*adapter*`、`examples/*`、`benchmark/*`。

边界：Astro 是站点执行/构建层，不负责 CMS 管理、内容治理和 Agent 生产状态。

### 5.10 Docusaurus

**定位与成熟度：完整专项产品；文档与知识站静态生成器。**

代码已证实的功能：

- Markdown/MDX 文档、页面和博客；
- 文档版本、语言国际化和版本切换；
- Sidebar、Navbar、TOC、标签和分页；
- Blog 作者、归档、Feed 和标签；
- 搜索适配、Sitemap、SEO 和 Client Redirect；
- Theme/Plugin/Preset 架构及 Swizzle 组件定制；
- Broken Link 检查、代码高亮和静态资源处理；
- CLI 创建、开发、构建和部署站点。

主要证据：`packages/docusaurus-plugin-content-docs/*`、`content-blog/*`、`theme-classic/*`、`core/*`、`mdx-loader/*`、`create-docusaurus/*`。

边界：适合 Documentation/Knowledge Site，不是可视化 CMS 或通用 AGC 站点生成系统。

---

## 6. 跨项目功能矩阵

| 能力 | 代表项目 | 代码体现 |
|---|---|---|
| 账号画像/品牌上下文 | Easel | profile 模板、平台与风格记忆 |
| 多源内容研究 | social-media-agent、fengyun-publish | loader、去重、验证、热点采集 |
| Agent 内容流水线 | Easel、OpenMontage、NarraLume | Skills/Graph/Worker/工具编排 |
| 人工审核 Agent 修改 | WordBird、social-media-agent | Diff review、interrupt node |
| 内容质量门禁 | fengyun-publish、OpenMontage | Critic、Gate、质量评分 |
| 多平台发布 | Postiz、Mixpost、Socioboard | Provider/Integration/Job |
| 发布不确定性处理 | Postiz、Mixpost | Job 状态、失败与重试，但治理深度不同 |
| 自动短视频 | MoneyPrinterTurbo | 脚本、素材、TTS、字幕、合成 |
| 专业生成媒体图 | ComfyUI | 节点 DAG 与模型执行 |
| 程序化视频 | Remotion | React 时间线与多环境渲染 |
| 自动剪辑 | auto-editor | 信号分析到时间线变换 |
| 写作结构/稿件分离 | novelWriter、WordBird、NarraLume | Outline、Chapter、Manuscript |
| Canon/连续性 | NarraLume、WordBird | Canon Candidate、Continuity Service |
| 结构化小说设计 | bibisco | 人物问卷、事件、关系、场景 |
| RAG 研究写作 | Synthetix、xnovelist | 文档管线、Embedding、检索 |
| Agent 外部接入 | Story Studio、Headcode、Caelo | MCP |
| 内容模型与关系 | Payload、Directus、Strapi | Field/Collection/Relation |
| Draft/Version/Publish | Payload、Strapi、Caelo | Draft、Version、Preview、Deploy |
| 文件化站点规范 | SiteSpec | Schema + YAML + Build |
| SEO 内容运营 | openSEO | 研究、生成、发布、监测 |
| 静态/SSR 站点 Runtime | Astro | 路由、构建、Adapter |
| 文档站版本化 | Docusaurus | Docs version/i18n/plugin |

---

## 7. 对 Digger 功能建模最有价值的代码事实

### 7.1 Social Domain

实际项目普遍证明 Social 不只是“生成一段文案”，而需要：

```text
Source/Research
→ Idea/Plan
→ Content
→ Platform Variant
→ Media
→ Schedule
→ Publication Attempt
→ External Result
→ Analytics
```

Postiz/Mixpost/Socioboard 证明 `Content`、`Account`、`Schedule`、`Publication` 和 `Analytics` 必须分开；同一内容面向多个平台时，平台 Variant 和发布结果不能合并成一个状态。

### 7.2 Writing Domain

WordBird、NarraLume、novelWriter 和 bibisco 共同证明至少需要区分：

```text
Work
Structure
Story Bible / Canon
Manuscript
Revision
Compile / Export
```

NarraLume 的 Canon Candidate 与 WordBird 的 Agent Diff Review 特别支持 Digger 的结论：Agent 输出不应直接进入 Official State。

### 7.3 Site Domain

Payload、Directus、Strapi、Caelo 和 SiteSpec 共同证明站点领域至少包含：

```text
Schema / Content Model
Entry / Page
Relation
Presentation
Draft / Version
Preview
Build / Deploy
Published State
```

Astro 证明 Build Runtime 与 CMS Domain 可以独立；Caelo 证明静态生成、Staging Preview 和 Deploy Run 应作为生产/发布过程，而不是简单字段。

### 7.4 Production Runtime

ComfyUI、LangGraph Social Agent、OpenMontage 和 Remotion 体现了三种不同 Runtime：

- DAG 执行 Runtime；
- Agent/状态图 Runtime；
- 媒体渲染 Runtime。

Digger 不应把它们统一成一个万能 Workflow 对象，而应通过 Capability、Action、Result、WorkingState 和 Change 协调。

### 7.5 Governance

最值得吸收的实际模式：

- WordBird：Agent Diff 先审核再应用；
- NarraLume：Canon Candidate 有接受/拒绝/撤回；
- fengyun-publish：独立 Critic 与物理 Gate；
- Caelo/Strapi：Reviewer 权限与 Review Workflow；
- Postiz/Mixpost：外部发布由持久化 Job 执行。

这些代码事实支持把生成、候选变更、治理决策、正式 Commit 和外部动作分开。

---

## 8. 未分析项与限制

- `SupoAI/supoclip` 仓库当前不存在，未纳入功能结论。
- 本次使用浅克隆的当前主分支，不代表历史版本或商业云版本。
- 未为 33 个项目安装依赖并执行全部测试；“已证实”表示存在明确实现和/或测试代码，不等同于已在本机完成端到端验收。
- 第三方平台 API、模型 API、Cookie 和商业许可证可能影响功能能否立即运行。
- 仓库内付费版文档、Roadmap 和仅有 UI 图标但没有适配逻辑的项目，未作为确定功能依据。

