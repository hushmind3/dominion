/** Native Rust goal admission payload shared by the beginner launchpad. */
export const DEFAULT_GOAL_BUDGET = 150_000;
export function createGoalPayload(input, kind = 'general', tokenBudget = DEFAULT_GOAL_BUDGET) {
  if (typeof input !== 'string') throw new TypeError('작업 내용을 입력해 주세요.');
  const goal = input.trim();
  if (!goal) throw new TypeError('맡길 작업을 입력해 주세요.');
  if ([...goal].length > 16_000) throw new RangeError('작업 내용은 16,000자 이내로 입력해 주세요.');
  if (kind !== 'general' && kind !== 'code') throw new TypeError('지원하지 않는 작업 유형입니다.');
  if (!Number.isSafeInteger(tokenBudget) || tokenBudget < 1_000 || tokenBudget > 2_000_000)
    throw new RangeError('사용량 상한은 1,000~2,000,000 토큰 사이의 정수여야 합니다.');
  return {
    goal,
    isolateWorktree: kind === 'code',
    requireTests: kind === 'code',
    tokenBudget,
    maxIterations: 16,
  };
}
