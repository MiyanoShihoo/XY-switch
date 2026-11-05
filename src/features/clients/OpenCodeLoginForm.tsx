import { useState } from "react";
import { t } from "../../i18n/language";
import { Button, Field, Input, Select } from "../../ui";
import type { NativeLoginMethod, NativeLoginPrompt } from "../../types/clients";

export function visible(prompt: NativeLoginPrompt, inputs: Record<string, string>): boolean {
  if (!prompt.when) return true;
  const value = inputs[prompt.when.key];
  return value !== undefined && (prompt.when.op === "eq" ? value === prompt.when.value : value !== prompt.when.value);
}

export function currentInputs(method: NativeLoginMethod, values: Record<string, string>): Record<string, string> {
  const inputs: Record<string, string> = Object.create(null) as Record<string, string>;
  for (const prompt of method.prompts) {
    if (visible(prompt, inputs)) inputs[prompt.key] = Object.hasOwn(values, prompt.key) ? values[prompt.key] : (prompt.kind === "select" ? prompt.options[0]?.value ?? "" : "");
  }
  return inputs;
}

export function OpenCodeLoginForm({ methods, disabled, onStart, onCancel }: {
  methods: NativeLoginMethod[];
  disabled: boolean;
  onStart: (method: NativeLoginMethod, inputs: Record<string, string>) => void;
  onCancel: () => void;
}) {
  const [selected, setSelected] = useState(0);
  const [values, setValues] = useState<Record<string, string>>({});
  const method = methods[selected];
  if (!method) return <section className="client-login" role="status"><p>{t("当前原生提供商没有可用的 OAuth 登录方式。")}</p><Button variant="quiet" size="sm" onClick={onCancel}>{t("关闭")}</Button></section>;
  const inputs = currentInputs(method, values);
  const start = () => {
    try { onStart(method, inputs); }
    finally { setValues({}); }
  };
  return <section className="client-login client-login-form" aria-label={t("OpenCode 提供商登录")}>
    <Field label={t("提供商与登录方式")} htmlFor="opencode-login-method">
      <Select id="opencode-login-method" value={selected} disabled={disabled} onChange={event => { setSelected(Number(event.target.value)); setValues({}); }}>
        {methods.map((item, index) => <option key={JSON.stringify([item.providerId, item.methodId])} value={index}>{item.providerLabel} · {item.label}</option>)}
      </Select>
    </Field>
    {method.prompts.filter(prompt => visible(prompt, inputs)).map((prompt, index) => <Field key={prompt.key} label={prompt.message} htmlFor={`opencode-login-input-${index}`}>
      {prompt.kind === "select" ? <Select id={`opencode-login-input-${index}`} value={inputs[prompt.key]} disabled={disabled} onChange={event => setValues(currentInputs(method, { ...inputs, [prompt.key]: event.target.value }))}>
        {prompt.options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}
      </Select> : <Input id={`opencode-login-input-${index}`} type="password" autoComplete="off" spellCheck={false} maxLength={4096} value={inputs[prompt.key]} disabled={disabled} onChange={event => setValues(currentInputs(method, { ...inputs, [prompt.key]: event.target.value }))} />}
    </Field>)}
    <p>{t("登录输入仅保存在当前表单内存中，确认或关闭表单后清除。")}</p>
    <div className="client-actions"><Button size="sm" disabled={disabled} onClick={start}>{t("检查并授权登录")}</Button><Button variant="quiet" size="sm" disabled={disabled} onClick={() => { setValues({}); onCancel(); }}>{t("关闭")}</Button></div>
  </section>;
}

export function NativeCodeForm({ disabled, onSubmit }: { disabled: boolean; onSubmit: (code: string) => void }) {
  const [code, setCode] = useState("");
  const submit = () => { try { onSubmit(code); } finally { setCode(""); } };
  return <div className="client-code-form">
    <Field label={t("原生授权页面返回的授权码")} htmlFor="opencode-authorization-code">
      <Input id="opencode-authorization-code" type="password" autoComplete="off" spellCheck={false} maxLength={16384} value={code} disabled={disabled} onChange={event => setCode(event.target.value)} />
    </Field>
    <Button size="sm" disabled={disabled || !code.trim()} onClick={submit}>{t("检查并提交授权码")}</Button>
  </div>;
}
