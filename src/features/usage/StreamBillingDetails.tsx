import type { StreamBillingReport } from "../../types/model-audit";
import { auditGapLabel } from "../../lib/model-audit";

/** Rendered only after the user requests the per-response stream analysis. */
export function StreamBillingDetails({ billing }: { billing?: StreamBillingReport | null }) {
  // Legacy reports contain character counts, which cannot reconstruct UTF-8 bytes.
  const bytesKnown = billing?.visibleBytes != null && Number.isFinite(billing.visibleBytes) && billing.visibleBytes >= 0;
  const notComparable = !bytesKnown || billing?.status === "not_comparable";
  return (
    <div className="audit-stream-billing">
      <strong>整段输出与计费比较（统计）</strong>
      {!billing ? <p className="audit-untested">未测到：此记录没有整段比较数据，不能从旧记录补算。这不等于没问题。</p> : (
        <>
          <p className={notComparable ? "audit-untested" : billing.status === "contradiction" ? "audit-statistical" : "audit-clear"}>
            {notComparable ? "未测到 / 不可比" : billing.status === "contradiction" ? "帧数多于上报可见 token，存在条件性差异" : "在边界假设下未见反向矛盾"}
          </p>
          <dl className="audit-stream-metrics">
            <div><dt>文本增量帧</dt><dd>{billing.textFrames}</dd></div>
            <div><dt>上报可见 token / UTF-8 字节</dt><dd>{billing.visibleTokens ?? "未测到"} / {bytesKnown ? billing.visibleBytes : "未测到"}</dd></div>
            <div><dt>token / UTF-8 字节</dt><dd>{metric(bytesKnown ? billing.tokensPerByte : null)}</dd></div>
            <div><dt>UTF-8 字节 / 帧</dt><dd>{metric(bytesKnown ? billing.bytesPerFrame : null)}</dd></div>
            <div><dt>帧 / token</dt><dd>{metric(billing.framesPerToken)}</dd></div>
          </dl>
          {!bytesKnown && <p className="audit-untested">未记录 UTF-8 字节数；旧字符计数不能替代字节计数，这不等于没问题。</p>}
          {billing.gaps.length > 0 && <ul className="audit-untested">{billing.gaps.map((gap) => <li key={gap}>{auditGapLabel(gap)}</li>)}</ul>}
          {billing.limitations.length > 0 && <ul className="audit-limitations">{billing.limitations.map((limitation, index) => <li key={index}>{limitation}</li>)}</ul>}
        </>
      )}
      <p>可见 token 为上报输出扣除已报推理后的数值。SSE 可能把一个 token 拆成多帧，因此该比较不是硬证据，也不能直接认定虚报。</p>
      <p>比率受输出语言构成影响，优先在同一渠道内比较变化；跨渠道比较前须按语言构成分桶，不能直接比较绝对值。</p>
    </div>
  );
}

function metric(value: number | null): string { return value == null ? "未测到" : value.toFixed(3); }
