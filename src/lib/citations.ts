// @author ygw
// 引用角标处理:把正文里的 [n] 转成可点击的链接(代码块内保持原样)

/** 把代码块之外的 [n] 替换为 [n](cite:n) 形式的 Markdown 链接 */
export function linkCitations(text: string, maxN: number): string {
  return text
    .split("```")
    .map((seg, i) => {
      if (i % 2 === 1) return seg;
      return seg.replace(/\[(\d{1,3})\](?!\()/g, (m, d: string) => {
        const n = Number(d);
        return n >= 1 && n <= maxN ? `[${n}](cite:${n})` : m;
      });
    })
    .join("```");
}
