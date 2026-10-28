# Digger 浏览器面板 Task 0:技术探针结果

日期:2026-10-28。对应计划 `dozer/docs/superpowers/plans/2026-10-28-digger-browser-panel.md` Task 0。探针源码:`dozer/spike/browser-probe/`(一次性,自成工作区,随时可删)。环境:macOS 26.6.2,wry 0.55.1,iced 0.14,objc2 0.6 / objc2-web-kit 0.3。

方法:先**读 wry 0.55.1 源码**(静态结论),再写一个 iced + wry 子视图的小程序**实测**。自动模式只加载本地 `file://` 与回环地址,没有出网;没有在"代理拒绝"生效之前调用过摄像头。

## 一句话结论

| 探针 | 状态 | 对设计的影响 |
|------|------|--------------|
| S1 `file://` | ✅ **能用,不需要特殊加载路径** | Task 2 不必加 `loadFileURL` 分支;同目录子资源能加载;**页面不能 `fetch` 其它本地文件** |
| S2 子视图键盘 | ✅ **已测并经你确认:装一个标准 Edit 菜单即可,不需要注入任何脚本** | 打字、中文输入法本来就正常;没有 Edit 菜单时 ⌘A/⌘C/⌘V/⌘X 在 webview 里全都不能用,装了全都能用;iced 输入框的快捷键不受影响 |
| S3 权限 | ✅ **有隐患但目前够不着**;代理拒绝已验证可行 | 照计划装代理委托,当作纵深防御 |
| S4 iced 焦点读取 | ✅ **标准 iced 下可用** | Task 6 的焦点桥按计划做 |
| S5 层级 | ➖ 没做(已是项目既有定论) | — |
| S6 池成本 | ✅ 创建很便宜,销毁**不立刻**释放内存 | "切项目就销毁重建"可行 |

## S1:`file://` 与 URL 处理(实测)

**能加载。** 用 wry 的普通 `with_url("file://…")` 与 `load_url` 都直接渲染,**不需要** `loadFileURL:allowingReadAccessTo:`。对 `/tmp/browser-probe/a.html`:标题加载、同目录 `<link rel=stylesheet>` 生效(`body` 背景读回 `rgb(18, 52, 86)`)、同目录 `<img src=i.svg>` 触发 `onload`。

**页面不能读别的本地文件。** 页面里 `fetch('file:///tmp/browser-probe/other.txt')` 在所有用例下都是 `FETCH-ERR`。这是好事:`file://` 页面拿不到"顺手读本机其它文件"的能力(至少 `fetch` 这条路;`XMLHttpRequest`、`<iframe>`、`<form>` 等其它路径没有逐一测)。

**含空格与中文的 URL 没有 panic,会被自动百分号编码。** wry 的 `navigate_to_url` 里是 `NSURL::URLWithString(..).unwrap()`,静态上看对非法 URL 会 panic。实测(macOS 26)`file:///…/My File.html` 实际发出的是 `…/My%20File.html`,`中文.html` 变成 `%E4%B8%AD%E6%96%87.html`,`with_url` 与 `load_url` 两条路径都一样,没有 panic。**注意:这是新系统(NSURL 新版解析会自动编码)的行为,旧 macOS 可能返回 nil 然后 panic——我没有旧系统可测。** 建议宿主在交给 wry 前自己做一次百分号编码(很便宜),并在 ADR 里写明最低支持的 macOS 版本。

**两个会影响 UX 的发现:**
1. **导航失败没有任何回调。** 访问不可达的 `http://127.0.0.1:9/a b?x=1 y` 时,`on_page_load` 的 `Started`/`Finished` 都没有触发(只看到初始的 `about:blank`)。wry 0.55 没有"导航失败"事件。后果:地址栏已经显示了新网址,但页面还停在旧内容,宿主不知道出了错。需要一个宿主侧的策略(例如提交后一段时间内没收到 `Started` 就提示"无法打开")。
2. **没有 scheme 的垃圾输入什么都不会发生。** `exa mple.com` 直接给 wry 时没有任何导航。我们的地址栏会先补 `https://`,所以不会直接遇到,但 `load_url` 的调用方不能假设"调用成功就一定会导航"。

## S3:未信任页面的权限(静态 + 实测)

**静态发现(读源码,不会触发任何系统提示):** wry 0.55.1 的 `WryWebViewUIDelegate::request_media_capture_permission` 对**所有来源、所有类型**(摄像头/麦克风)**无条件** `decision_handler(WKPermissionDecision::Grant)`;没有 builder 开关,委托是私有类型。也就是说,一旦页面能调到 `getUserMedia`,唯一的保护只剩 macOS 的系统授权提示(每个应用一次;用户授权过一次,之后任何网页都静默通过)。

**实测:当前够不着。** 让页面真的调 `navigator.mediaDevices.getUserMedia`:在 `file://` 页面和 `http://127.0.0.1` 页面上,`navigator.mediaDevices` 都是 **`undefined`**(`isSecureContext=true`)。这个 WKWebView 配置默认没有暴露 `mediaDevices`,所以那段无条件 `Grant` 目前是**休眠的**。不过这依赖 wry/WebKit 的默认值,将来任何一方变化(或有人打开相应偏好)都会让它突然生效,所以仍应防御。

**代理委托可行,已验证:** 在 wry 建好 webview 之后,用 objc2 声明一个代理类(`DenyMediaProxy`),`setUIDelegate` 换掉原委托(WKWebView 对委托是 **weak** 引用,代理要自己保活);代理对媒体捕获返回 `Deny`,其余方法通过 `forwardingTargetForSelector:` 转发给 wry 的原委托,并重写 `respondsToSelector:` 如实回答。实测:代理已生效;对新窗口、文件选择、媒体捕获三个可选方法的 `respondsToSelector` 与原委托一致(都是 true,新窗口与文件选择仍由 wry 处理);直接以 WebKit 的方式调用代理的媒体捕获方法,决定是 `Deny`(2)。**没有能端到端证明"页面真实请求时 WebKit 确实走了代理"**,因为页面根本调不到 `getUserMedia`;所以这一层只验证到"代理装得上、转发正确、直接调用拒绝"。

**没测的:** 定位(`navigator.geolocation`)。wry 没有对应钩子,WKWebView 的定位依赖应用的系统授权与 Info.plist 用途说明,可能弹系统提示,我没有冒险去触发。

## S4:标准 iced 下的焦点读取(实测)

`operation::focus("a")` 后 `operation::is_focused("a")` 为 `true`、`is_focused("b")` 为 `false`;再 `focus("b")` 后 `a=false`、`b=true`。全部通过 `Task` 链完成,**不需要** Dozer 那种每帧运行的自定义事件环。所以地址栏焦点桥(`is_focused(addr_field_id())` → `State::set_addr_focused`)在标准 `iced::application` 下可行。没有测的:触发时机(鼠标点击 / 键盘 Tab / 窗口失焦之后是否一帧内就能读到正确值)——需要真实窗口交互,放进验收清单。

## S6:池成本(实测)

- **创建:** 8 个页签,每个约 **32–41 ms**。
- **内存:** 本进程 RSS 约 +3 MB;系统内 WebContent 进程 RSS 合计约 **+74 MB**(8 个页签;数字含其它应用的波动,仅作量级参考)。
- **销毁不立刻释放:** 全部销毁 1.5 秒后,WebContent 合计 RSS 几乎没变(1520800 → 1520736 KB)。WebKit 的进程是池化、延迟回收的。
- 对设计的意义:创建很便宜,所以"切项目就销毁、切回来重建"没有性能问题;但不要指望销毁能立刻降内存。

## S2:子视图里的键盘(实测,两遍对比)

做法:同一个探针,webview 里一个 `input`、一个 `textarea`,页面监听 `keydown`/`copy`/`paste`/`cut`/`compositionstart`/`compositionend`/`focus`,事件写进标题并由宿主原生标题回调打印;你在真实窗口里按固定顺序操作。第一遍**没有** Edit 菜单,第二遍装了标准 Edit 菜单(Cut/Copy/Paste/Select All,目标为空,走响应链)。

| 事件(页面收到的次数) | 没有 Edit 菜单 | 有 Edit 菜单 |
|------------------------|----------------|--------------|
| `copy` | **0** | **2** |
| `paste` | **0** | **4** |
| `cut` | **0** | **2** |
| `compositionend`(中文输入法提交) | 4 | 6 |
| 带 ⌘ 的 `keydown`(如 `c+meta`) | 26 | 10 |

**结论:**
1. **打字与中文输入法本来就正常**,两遍都一样:拼音输入、选词、提交都有成对的 `compositionstart`/`compositionend`。不需要为输入法做任何事。
2. **没有 Edit 菜单时,⌘C / ⌘V / ⌘X 在页面里完全不生效。** 页面能收到 `c+meta` 这类 `keydown`(所以更正我之前写的"JavaScript 也监听不到":那是读 wry 注释推出来的,**实测不对**),但编辑动作本身没有执行——`copy`/`paste`/`cut` 一个都没触发。这和 wry 的 `performKeyEquivalent:` 对子视图恒返回 `NO` 一致。
3. **装了 Edit 菜单就好了**:`copy`/`paste`/`cut` 在 `input` 和 `textarea` 里都正常触发,而且是**零脚本**——⌘ 组合键走菜单 → 响应链 → first responder(WKWebView)。顺带的现象:装了菜单后,被菜单接管的组合键(⌘C/⌘V/⌘X)页面**不再收到** `c+meta` 这类 `keydown`(只收到单独的 `Meta`),而没绑菜单的(如 `⌘←`)页面仍能收到。
4. **所以出路 A 成立,出路 B(注入带 nonce 的脚本桥)不需要。** 这对安全设计是好消息:浏览器子视图可以**完全不注入脚本、不装 IPC 处理器**,页面发不出任何消息给宿主。**更正(2026-10-28,Dozer 真机实测):** wry 仍会注入一个空的 `window.ipc` 壳,页面能看到它,但宿主这头没有处理器,`postMessage` 会抛错;这份报告原先写的是"页面看不到 `window.ipc`",不准确。

**用户确认的两项(日志看不出来的,由你肉眼验证):**
- ✅ **装了 Edit 菜单后,webview 里 ⌘A 能全选。** 没装菜单时,webview 里这些快捷键(⌘A / ⌘C / ⌘V / ⌘X)**全都不能用**。
- ✅ **Edit 菜单没有影响 iced 输入框**:窗口底部两个 iced 输入框里的 ⌘C / ⌘V / ⌘X / ⌘A 照常可用。(原因应当是:first responder 是 winit 的视图、不响应 `copy:` 等选择器时,菜单项被禁用,按键落到 `keyDown`——这是对现象的解释,不是单独验证过的。)

另外,Digger 自己没有安装任何主菜单(`setMainMenu` / `NSMenu::new` 在 Digger 代码里一处都没有),所以这个 Edit 菜单要由 Digger 新装。

## S5:层级与隐藏(没做)

"webview 恒在 iced 之上、原生菜单(NSMenu)是独立窗口所以盖得住 webview、iced 自绘浮层会被盖住所以必须隐藏 webview"是本项目 CLAUDE.md 与 X1 已有的定论,没有新探针的必要。留待验收清单里人工确认。

## 对计划的修改(已回写到计划)

1. **Task 2** 不加 `file://` 特殊加载分支;新增:宿主在交给 wry 前对 URL 做百分号编码;新增"导航失败无回调"的宿主侧超时提示;媒体捕获代理委托(`objc2-web-kit` 依赖)。
2. **Task 6** 的焦点桥按原设计做(S4 已验证 API)。
3. **S2 已决:** 不注入任何脚本;Digger 新装一个标准 Edit 菜单(见下),并在验收清单里加"iced 输入框的 ⌘ 组合键不受影响"。
4. **验收清单 I 区**补:`file://` 页面的相对资源、页面读不到其它本地文件、导航失败的提示、焦点点击时机。
