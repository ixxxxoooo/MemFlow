// @author ygw
// 条目相关错误判断

/** 后端读取不存在的条目时返回的提示文案包含「记录不存在」 */
export function isMissingItemError(message: string): boolean {
  return message.includes("记录不存在");
}
