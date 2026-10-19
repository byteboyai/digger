//! 预览 webview 句柄表与期望清单之间的纯差分(S5b-2 P3 Task 7)。
//!
//! 这里**不碰 wry**:只回答"相对当前存活句柄,为了让页面变成 `desired` 的样子,
//! 该建、该改、该拆哪些"。宿主 `preview_native.rs` 在窗口线程上按顺序执行这些
//! 操作;纯函数使得差分逻辑可以无头测试,也让"什么时候会漏拆句柄"一眼可见。
//!
//! 规格要点:
//! - `url` 只在真的变化时才给 `Some`(换主题/重载会改 `_r` 查询串),避免每次
//!   同步都让 WKWebView 重新导航;
//! - `park_offscreen` 的宿主"未显示"时仍保持 `visible = true`——WKWebView 对
//!   hidden 视图会挂起 rAF,渲染器(docx 分页)会因此死等(见
//!   `PreviewWebviewSpec::park_offscreen`);
//! - 句柄表与章节编辑器各自独立(规格 Q1):本模块只看预览的 `live`,不读
//!   编辑器的表。

use std::collections::BTreeMap;

use bytehost_core::LogicalRect;
use dozer_panel_preview::PreviewWebviewSpec;

/// 一条存活句柄的最小记账(不持有 wry 句柄本身)。
#[derive(Debug, Clone, PartialEq)]
pub struct LiveView {
    pub url: String,
    pub rect: LogicalRect,
    pub visible: bool,
}

/// 对单个句柄的一次操作。`Update` 的三个字段都表示"要设成的目标值",宿主只在
/// 有差异时真正调用 wry。
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Create {
        spec: PreviewWebviewSpec,
        rect: LogicalRect,
    },
    Update {
        id: usize,
        url: Option<String>,
        rect: LogicalRect,
        visible: bool,
    },
    Destroy {
        id: usize,
    },
}

/// 该 spec 在页面上应当表现成"可见"还是"藏起"。
fn desired_visible(spec: &PreviewWebviewSpec) -> bool {
    spec.visible || spec.park_offscreen
}

/// 算出差分。顺序:先 `Destroy`(腾出预算/避免 id 复用冲突),再按 `desired` 的
/// 顺序 `Create`/`Update`。
pub fn plan(
    live: &BTreeMap<usize, LiveView>,
    desired: &[(PreviewWebviewSpec, LogicalRect)],
) -> Vec<Op> {
    let mut ops = Vec::new();
    let wanted: std::collections::HashSet<usize> = desired.iter().map(|(s, _)| s.id).collect();
    for id in live.keys() {
        if !wanted.contains(id) {
            ops.push(Op::Destroy { id: *id });
        }
    }
    for (spec, rect) in desired {
        let visible = desired_visible(spec);
        match live.get(&spec.id) {
            None => ops.push(Op::Create {
                spec: spec.clone(),
                rect: *rect,
            }),
            Some(current) => {
                let url = (current.url != spec.url).then(|| spec.url.clone());
                if url.is_some() || current.rect != *rect || current.visible != visible {
                    ops.push(Op::Update {
                        id: spec.id,
                        url,
                        rect: *rect,
                        visible,
                    });
                }
            }
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> LogicalRect {
        LogicalRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn spec(id: usize, url: &str, visible: bool, parked: bool) -> PreviewWebviewSpec {
        PreviewWebviewSpec {
            id,
            url: url.to_string(),
            visible,
            editor_binding: None,
            reserve: None,
            loading_generation: None,
            park_offscreen: parked,
        }
    }

    fn live(url: &str, rect: LogicalRect, visible: bool) -> LiveView {
        LiveView {
            url: url.to_string(),
            rect,
            visible,
        }
    }

    #[test]
    fn empty_to_empty_is_no_ops() {
        assert!(plan(&BTreeMap::new(), &[]).is_empty());
    }

    #[test]
    fn new_spec_creates() {
        let ops = plan(
            &BTreeMap::new(),
            &[(
                spec(3, "dozer://flyfish/a", false, false),
                rect(0.0, 0.0, 1.0, 1.0),
            )],
        );
        assert!(matches!(&ops[..], [Op::Create { spec, .. }] if spec.id == 3));
    }

    #[test]
    fn url_change_updates_only_url() {
        let mut map = BTreeMap::new();
        let r = rect(1.0, 2.0, 3.0, 4.0);
        map.insert(1, live("dozer://flyfish/a", r, true));
        let ops = plan(&map, &[(spec(1, "dozer://flyfish/a?_r=2", true, false), r)]);
        match &ops[..] {
            [
                Op::Update {
                    id,
                    url,
                    rect: rr,
                    visible,
                },
            ] => {
                assert_eq!(*id, 1);
                assert_eq!(url.as_deref(), Some("dozer://flyfish/a?_r=2"));
                assert_eq!(*rr, r);
                assert!(*visible);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn removed_spec_destroys() {
        let mut map = BTreeMap::new();
        map.insert(9, live("dozer://flyfish/a", rect(0.0, 0.0, 1.0, 1.0), true));
        let ops = plan(&map, &[]);
        assert_eq!(ops, vec![Op::Destroy { id: 9 }]);
    }

    #[test]
    fn rect_only_change_updates_rect_without_url() {
        let mut map = BTreeMap::new();
        map.insert(1, live("dozer://flyfish/a", rect(0.0, 0.0, 1.0, 1.0), true));
        let ops = plan(
            &map,
            &[(
                spec(1, "dozer://flyfish/a", true, false),
                rect(0.0, 0.0, 2.0, 2.0),
            )],
        );
        match &ops[..] {
            [Op::Update { url, rect: rr, .. }] => {
                assert!(url.is_none());
                assert_eq!(*rr, rect(0.0, 0.0, 2.0, 2.0));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parked_host_stays_visible_when_not_shown() {
        // 规格:`park_offscreen` 的宿主即使 `visible=false` 也必须以 visible 创建并保持。
        let ops = plan(
            &BTreeMap::new(),
            &[(
                spec(1, "dozer://flyfish/a", false, true),
                rect(0.0, 0.0, 1.0, 1.0),
            )],
        );
        assert!(matches!(&ops[..], [Op::Create { .. }]));

        let mut map = BTreeMap::new();
        map.insert(1, live("dozer://flyfish/a", rect(0.0, 0.0, 1.0, 1.0), true));
        let ops = plan(
            &map,
            &[(
                spec(1, "dozer://flyfish/a", false, true),
                rect(0.0, 0.0, 1.0, 1.0),
            )],
        );
        assert!(ops.is_empty(), "停车态不产生隐藏操作: {ops:?}");
    }

    #[test]
    fn hiding_a_ready_host_updates_visibility() {
        let mut map = BTreeMap::new();
        map.insert(1, live("dozer://flyfish/a", rect(0.0, 0.0, 1.0, 1.0), true));
        let ops = plan(
            &map,
            &[(
                spec(1, "dozer://flyfish/a", false, false),
                rect(0.0, 0.0, 1.0, 1.0),
            )],
        );
        assert!(matches!(&ops[..], [Op::Update { visible: false, .. }]));
    }

    #[test]
    fn destroy_all_lists_every_live_id() {
        let mut map = BTreeMap::new();
        map.insert(1, live("dozer://flyfish/a", rect(0.0, 0.0, 1.0, 1.0), true));
        map.insert(5, live("dozer://editor/b", rect(0.0, 0.0, 1.0, 1.0), true));
        let ops = plan(&map, &[]);
        assert_eq!(ops.len(), 2);
        assert!(ops.iter().all(|op| matches!(op, Op::Destroy { .. })));
    }
}
