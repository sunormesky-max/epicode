export type LibraryRequestAction = 'accepted' | 'rejected';

/**
 * 收集请求的处理备注。
 *
 * 返回 `null` 表示用户在 prompt 里点了"取消" —— 调用方应直接放弃本次操作;
 * 返回字符串(可能为空串)表示用户点了"确定",空串表示"跳过备注"。
 */
export function promptRequestNote(
  action: LibraryRequestAction,
  ask: (message: string) => string | null,
): string | null {
  const message = action === 'accepted' ? '处理备注(可选, 回车跳过):' : '拒绝原因(可选):';
  return ask(message);
}
