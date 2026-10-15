// @author ixxxxoooo
// 代码围栏辅助:流式输出时判断某个代码块是否已经写完(闭合)

/**
 * 判断一个围栏代码块的源码是否已闭合
 *
 * 参数: blockSource 该块在 Markdown 源文中的原文(含起始围栏)
 * 返回: 最后一行是单独的围栏(``` 或 ~~~)才算闭合;流式中途只有起始围栏与半截内容则为 false
 */
export function isFenceClosed(blockSource: string): boolean {
  const lines = blockSource.replace(/\s+$/, "").split("\n");
  if (lines.length < 2) return false;
  const open = lines[0].trim().match(/^(`{3,}|~{3,})/);
  if (!open) return false;
  const closer = lines[lines.length - 1].trim();
  const ch = open[1][0];
  return closer.length >= open[1].length && [...closer].every((c) => c === ch);
}
