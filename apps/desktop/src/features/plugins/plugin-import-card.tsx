type PluginImportCardProps = {
  dragging: boolean;
  isUploading: boolean;
  uploadError: string | null;
  onRequestSelectFile: () => void;
  onDraggingChange: (dragging: boolean) => void;
  onImportFile: (file: File | null) => void;
};

export function PluginImportCard({
  dragging,
  isUploading,
  uploadError,
  onRequestSelectFile,
  onDraggingChange,
  onImportFile,
}: PluginImportCardProps) {
  return (
    <article className="ui-card">
      <div className="ui-card-header">
        <div>
          <h3 className="ui-card-title">插件导入</h3>
          <p className="ui-card-desc">支持拖拽或选择 ZIP，导入失败会显示明确错误原因。</p>
        </div>
      </div>

      <div className="ui-card-body">
        <div
          role="button"
          tabIndex={0}
          className={`ui-focus cursor-pointer rounded-lg border border-dashed px-4 py-4 transition ${
            dragging
              ? "border-[var(--accent-strong)] bg-[var(--accent-soft)]/25"
              : "border-[var(--panel-border)] bg-[var(--panel-muted)]/35"
          }`}
          onClick={onRequestSelectFile}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              onRequestSelectFile();
            }
          }}
          onDragEnter={(event) => {
            event.preventDefault();
            onDraggingChange(true);
          }}
          onDragOver={(event) => {
            event.preventDefault();
            onDraggingChange(true);
          }}
          onDragLeave={(event) => {
            event.preventDefault();
            onDraggingChange(false);
          }}
          onDrop={(event) => {
            event.preventDefault();
            onDraggingChange(false);
            onImportFile(event.dataTransfer.files?.[0] ?? null);
          }}
        >
          <p className="text-sm text-[var(--app-text)]">
            拖拽 `.zip` 插件包到此处，或点击这里选择文件。
          </p>
          <p className="mt-1 text-xs text-[var(--muted-text)]">导入后会自动刷新插件列表。</p>
          {isUploading && (
            <p className="mt-2 text-xs text-[var(--accent-strong)]">插件导入中，请稍候…</p>
          )}
          {uploadError && (
            <p className="mt-2 rounded-md border border-rose-400/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-300">
              {uploadError}
            </p>
          )}
        </div>
      </div>
    </article>
  );
}
