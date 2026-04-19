import { Skeleton } from "../../components/skeleton";
import { formatTimestamp as formatTimestampValue, statusToneClass } from "../../lib/ui";
import type { PluginRecord } from "../../types/core";

type PluginListCardProps = {
  loading: boolean;
  plugins: PluginRecord[];
  expandedPluginId: string | null;
  activePluginId: string | null;
  onToggleExpanded: (pluginId: string) => void;
  onDelete: (plugin: PluginRecord) => void;
};

export function PluginListCard({
  loading,
  plugins,
  expandedPluginId,
  activePluginId,
  onToggleExpanded,
  onDelete,
}: PluginListCardProps) {
  return (
    <article className="ui-card">
      <div className="ui-card-header">
        <div>
          <h3 className="ui-card-title">插件列表</h3>
          <p className="ui-card-desc">查看版本与状态，并执行删除等管理动作。</p>
        </div>
      </div>

      <div className="ui-card-body">
        {loading ? (
          <div className="space-y-3">
            <Skeleton className="h-28" />
            <Skeleton className="h-28" />
          </div>
        ) : plugins.length === 0 ? (
          <p className="text-sm text-[var(--muted-text)]">暂无插件，请先导入插件包。</p>
        ) : (
          <div className="space-y-3">
            {plugins.map((plugin) => {
              const expanded = expandedPluginId === plugin.id;
              const busy = activePluginId === plugin.id;
              return (
                <article
                  key={plugin.id}
                  className="rounded-lg border border-[var(--panel-border)] bg-[var(--panel-bg)]/60 p-3"
                >
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div>
                      <h4 className="text-base font-semibold text-[var(--app-text)]">
                        {plugin.name}
                      </h4>
                      <p className="mt-1 text-xs text-[var(--muted-text)]">{plugin.plugin_id}</p>
                      <p className="mt-1 text-xs text-[var(--muted-text)]">
                        版本 {plugin.version} · spec {plugin.spec_version} · 类型{" "}
                        {plugin.plugin_type}
                      </p>
                    </div>
                    <div className="flex w-full flex-wrap items-center gap-2 md:w-auto">
                      <span className={`ui-badge ${statusToneClass(plugin.status)}`}>
                        {plugin.status}
                      </span>
                      <button
                        type="button"
                        aria-expanded={expanded}
                        className="ui-btn ui-btn-secondary ui-focus"
                        onClick={() => onToggleExpanded(plugin.id)}
                      >
                        {expanded ? "收起详情" : "查看详情"}
                      </button>
                      <button
                        type="button"
                        className="ui-btn ui-btn-danger ui-focus"
                        disabled={busy}
                        onClick={() => onDelete(plugin)}
                      >
                        删除
                      </button>
                    </div>
                  </div>

                  {expanded && (
                    <dl className="mt-3 grid gap-2 rounded-lg border border-[var(--panel-border)] bg-[var(--panel-bg)]/55 p-3 text-xs md:grid-cols-2">
                      <DetailRow label="插件记录 ID" value={plugin.id} />
                      <DetailRow label="插件标识" value={plugin.plugin_id} />
                      <DetailRow label="类型" value={plugin.plugin_type} />
                      <DetailRow label="状态" value={plugin.status} />
                      <DetailRow
                        label="安装时间"
                        value={formatTimestamp(plugin.installed_at)}
                      />
                      <DetailRow
                        label="更新时间"
                        value={formatTimestamp(plugin.updated_at)}
                      />
                    </dl>
                  )}
                </article>
              );
            })}
          </div>
        )}
      </div>
    </article>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-[var(--muted-text)]">{label}</dt>
      <dd className="mt-1 break-all text-[var(--app-text)]">{value}</dd>
    </div>
  );
}

function formatTimestamp(value: string): string {
  return formatTimestampValue(value, value);
}
