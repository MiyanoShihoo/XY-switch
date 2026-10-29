import { useLanguage, t } from "../i18n/language";
import { useState } from "react";
import { Button, InlineError, Modal } from "../ui";
import { ProviderAdvancedEditor } from "../features/providers/ProviderAdvancedEditor";
import { createProviderAdvanced } from "../lib/providers";
import { parseBodyOverrides, parseHeaderOverrides, validateProviderAdvanced } from "../lib/provider-advanced";
import type { ProviderAdvancedConfig } from "../types/provider";
import "../features/providers/providers.css";

/** DEV-only host for the actual mapping editor. State is deliberately memory-only. */
export function ProviderModelQaHost() {
  useLanguage();
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState<ProviderAdvancedConfig>(() => ({ ...createProviderAdvanced(), modelMappings: [{ model: "qa-alpha", displayName: "Alpha", contextWindow: 128000 }, { model: "qa-beta", displayName: "Beta", contextWindow: 200000 }] }));
  const [headers, setHeaders] = useState("{}");
  const [body, setBody] = useState("{}");
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");
  const [discovered, setDiscovered] = useState(false);
  const save = () => {
    try {
      const next = { ...value, headerOverrides: parseHeaderOverrides(headers), bodyOverrides: parseBodyOverrides(body) };
      const problem = validateProviderAdvanced(next); if (problem) throw new Error(problem);
      setSaved(JSON.stringify(next, null, 2)); setOpen(false);
    } catch (cause) { setError(String(cause)); }
  };
  return <main className="workspace-page"><h1>{t("模型编辑验收")}</h1><p className="notice">{t("使用实际模型映射控件；测试数据只在内存中，不连接服务、不保存密钥或修改本机配置。")}</p><div className="inline-actions"><Button onClick={() => { setError(""); setOpen(true); }}>{t("打开模型编辑器")}</Button><Button variant="secondary" onClick={() => { document.documentElement.dataset.theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark"; }}>{t("切换主题")}</Button></div>{saved && <pre data-testid="qa-model-snapshot">{saved}</pre>}<Modal open={open} title={t("模型映射验收")} size="lg" onClose={() => setOpen(false)} footer={<><Button variant="secondary" onClick={() => setOpen(false)}>{t("取消")}</Button><Button onClick={save}>{t("保存测试结果")}</Button></>}><div className="form-stack"><InlineError message={error} /><ProviderAdvancedEditor value={value} onChange={setValue} headerText={headers} bodyText={body} onHeader={setHeaders} onBody={setBody} models={discovered ? Array.from({ length: 40 }, (_, i) => ({ id: "qa-discovered-" + i, name: t("测试模型 ") + i })) : []} onDiscover={() => setDiscovered(true)} modelBusy={false} disabled={false} apiKeyConfigured /></div></Modal></main>;
}
