//! Digger 的产品信任策略(Task 14;规格 §6.3)。
//!
//! **信任决策只在产品适配层做,不在 bytehost、不在界面动作里。** `bytehost-apps` 只是把产品给的
//! [`Provenance`] / [`TrustLevel`] 记进计划并如实展示;真正的"这条来源该按几级信任处理、能不能装"
//! 是 Digger 的产品判断。本模块就是那唯一的判断点——纯函数,不碰 IO、不碰界面,便于单测钉死。
//!
//! 一期策略(规格 §6.3 / 计划 Task 14 clause 3–4):
//! - **本机目录 / 本机压缩包** → 用户自己挑的本地来源,[`Provenance::Local`] + [`TrustLevel::Trusted`];
//!   经人类明确批准后可装进程型应用(`static_web` / `node` / `python`)。
//! - **https URL 来源** → [`Provenance::ThirdParty`] + [`TrustLevel::Untrusted`];服务端也会按实际
//!   来源再校正一遍(URL 一律 ThirdParty/Untrusted)。**不受信任只允许 `static_web`**,进程型一律拒绝。
//! - **Agent 生成**若将来接入,同样按 `Untrusted` 处理,规则与 URL 相同。
//!
//! "Agent 与应用页面不能构造有效的人类审批事件"由两件事共同保证:(1) 审批消息只从产品的原生界面
//! 发出;(2) 审批绑定**用户看到的这一份计划**(`bytehost-panel` 的 `ApproveClicked` 原样 `approve`
//! 展示的 `InstallPlan`,服务端再逐字段核对)。本模块只负责"展示什么、允许不允许",不负责发审批事件。

use bytehost_apps::plan::{InstallPlan, Provenance, TrustLevel};
use bytehost_panel::install::SourceChoice;

/// 一条来源对应的产品策略:记进计划的 `provenance` / `trust`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePolicy {
    pub provenance: Provenance,
    pub trust: TrustLevel,
}

impl SourcePolicy {
    /// 本机来源(目录 / 压缩包):Local + Trusted。
    pub const LOCAL_TRUSTED: Self = Self {
        provenance: Provenance::Local,
        trust: TrustLevel::Trusted,
    };

    /// 网络 / Agent 来源:ThirdParty + Untrusted。
    pub const THIRD_PARTY_UNTRUSTED: Self = Self {
        provenance: Provenance::ThirdParty,
        trust: TrustLevel::Untrusted,
    };
}

/// 产品对某条安装来源的策略(纯函数)。
pub fn policy_for(choice: SourceChoice) -> SourcePolicy {
    match choice {
        SourceChoice::Dir | SourceChoice::Archive => SourcePolicy::LOCAL_TRUSTED,
        SourceChoice::Url => SourcePolicy::THIRD_PARTY_UNTRUSTED,
    }
}

/// 一条已生成的安装计划过产品策略门的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Review {
    /// 允许展示给用户审批(是否真的装仍要人类点确认)。
    Allowed,
    /// 明确拒绝:不该展示审批卡,`reason` 给人看。
    Rejected(String),
}

impl Review {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Review::Allowed)
    }
}

/// 受信任的进程型运行方式(规格 §6.3:本机可信应用本轮覆盖 Python / Node)。
const PROCESS_RUNTIMES: &[&str] = &["node", "python"];

/// 产品对一份**已生成**的安装计划做策略复核(纯函数)。
///
/// 与 [`policy_for`] 的区别:计划里带着服务端解析出的**真实** `provenance` / `trust` / `runtime_kind`
/// (URL 来源服务端会校正成 ThirdParty/Untrusted),这里据实判断能不能进审批。
/// - `Untrusted` 只允许 `static_web`;进程型拒绝。
/// - 已知运行方式(`static_web`/`node`/`python`)之外的一律拒绝(不静默放行未知)。
pub fn review(plan: &InstallPlan) -> Review {
    let kind = plan.runtime_kind.as_str();
    match plan.trust {
        TrustLevel::Untrusted if PROCESS_RUNTIMES.contains(&kind) => Review::Rejected(format!(
            "不受信任的来源({})只能装静态网页(static_web),不能装需要本机进程的 {} 应用;请改用本机来源并明确批准。",
            provenance_label(plan.provenance),
            kind,
        )),
        TrustLevel::Untrusted if kind != "static_web" => Review::Rejected(format!(
            "不认识的运行方式 {kind}:不受信任来源只允许 static_web。"
        )),
        _ if !(kind == "static_web" || PROCESS_RUNTIMES.contains(&kind)) => {
            Review::Rejected(format!("暂不支持 {kind} 类型的应用。"))
        }
        _ => Review::Allowed,
    }
}

fn provenance_label(p: Provenance) -> &'static str {
    match p {
        Provenance::Local => "本机",
        Provenance::AgentGenerated => "Agent 生成",
        Provenance::ThirdParty => "第三方",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_sources_are_trusted_and_network_is_not() {
        assert_eq!(
            policy_for(SourceChoice::Dir),
            SourcePolicy {
                provenance: Provenance::Local,
                trust: TrustLevel::Trusted
            }
        );
        assert_eq!(
            policy_for(SourceChoice::Archive),
            SourcePolicy::LOCAL_TRUSTED
        );
        assert_eq!(
            policy_for(SourceChoice::Url),
            SourcePolicy::THIRD_PARTY_UNTRUSTED
        );
    }

    fn plan(kind: &str, trust: TrustLevel, provenance: Provenance) -> InstallPlan {
        crate::testing::plan("demo", (1, 0, 0), kind, trust, provenance)
    }

    #[test]
    fn a_trusted_local_process_app_is_allowed_to_be_reviewed() {
        assert_eq!(
            review(&plan("python", TrustLevel::Trusted, Provenance::Local)),
            Review::Allowed
        );
        assert_eq!(
            review(&plan("node", TrustLevel::Trusted, Provenance::Local)),
            Review::Allowed
        );
        assert_eq!(
            review(&plan("static_web", TrustLevel::Trusted, Provenance::Local)),
            Review::Allowed
        );
    }

    #[test]
    fn an_untrusted_process_app_is_rejected_only_static_web_is_allowed() {
        assert!(
            review(&plan(
                "static_web",
                TrustLevel::Untrusted,
                Provenance::ThirdParty
            ))
            .is_allowed()
        );
        assert!(matches!(
            review(&plan(
                "python",
                TrustLevel::Untrusted,
                Provenance::ThirdParty
            )),
            Review::Rejected(_)
        ));
        assert!(matches!(
            review(&plan("node", TrustLevel::Untrusted, Provenance::ThirdParty)),
            Review::Rejected(_)
        ));
    }

    #[test]
    fn an_unknown_runtime_is_never_silently_allowed() {
        assert!(matches!(
            review(&plan("container", TrustLevel::Trusted, Provenance::Local)),
            Review::Rejected(_)
        ));
        assert!(matches!(
            review(&plan(
                "container",
                TrustLevel::Untrusted,
                Provenance::ThirdParty
            )),
            Review::Rejected(_)
        ));
    }

    #[test]
    fn agent_generated_is_treated_like_network() {
        // Agent 生成来源按 Untrusted 处理,规则与 URL 相同(policy_for 无 Agent 项;这里直接验证 review)。
        assert!(matches!(
            review(&plan(
                "python",
                TrustLevel::Untrusted,
                Provenance::AgentGenerated
            )),
            Review::Rejected(_)
        ));
        assert!(
            review(&plan(
                "static_web",
                TrustLevel::Untrusted,
                Provenance::AgentGenerated
            ))
            .is_allowed()
        );
    }
}
