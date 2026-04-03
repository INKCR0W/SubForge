Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action
    )

    Write-Host "==> $Name"
    $startedAt = Get-Date
    & $Action
    $elapsed = (Get-Date) - $startedAt
    Write-Host ("    完成，耗时 {0:n1}s" -f $elapsed.TotalSeconds)
}

Invoke-Step -Name "管理 API 真实子进程链路回归" -Action {
    cargo test -p subforge-core --test management_api_process -- --nocapture
}

Invoke-Step -Name "无头模式 run -c 真实子进程链路回归" -Action {
    cargo test -p subforge-core --test headless_run_config_process -- --nocapture
}

Invoke-Step -Name "后端管理链路 e2e（in-process）" -Action {
    cargo test -p app-http-server e2e_import_source_refresh_and_raw_profile_output -- --nocapture
}

Invoke-Step -Name "聚合性能基线（1000 节点 < 500ms）" -Action {
    cargo test -p app-aggregator aggregates_1000_nodes_within_500ms -- --nocapture
}

Invoke-Step -Name "全仓 clippy 零 warning 闸门" -Action {
    cargo clippy --workspace --all-targets -- -D warnings
}

Write-Host "E2E Gate 校验通过。"
