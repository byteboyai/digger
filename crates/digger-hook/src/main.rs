//! digger-hook 薄入口:传 Digger 的产品身份给 dozer-hook 的 run_main。

fn main() {
    dozer_hook::run_main(digger_paths::product());
}
