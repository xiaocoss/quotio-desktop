import { describe, it, expect } from "vitest";
import { servingFile } from "./format";
import type { AuthFile } from "../types";

function af(name: string, partial: Partial<AuthFile>): AuthFile {
  return {
    id: name,
    name,
    provider: "codex",
    status: "ok",
    disabled: false,
    unavailable: false,
    ...partial,
  } as AuthFile;
}

describe("servingFile", () => {
  it("按优先级取第一个健康号,不被低优先级号的高累计成功带偏", () => {
    // 顺序 [a, b]:a 刚恢复(成功少),b 之前吃了一波(成功多)。应返回 a —— 优先级更高且健康,
    // 这是代理 fill-first 真正会先用的号。
    const files = [af("a", { success: 5, failed: 0 }), af("b", { success: 50, failed: 0 })];
    expect(servingFile(["a", "b"], files)).toBe("a");
  });

  it("跳过净失败(success<=failed,上游不稳/限流)的号,落到下一个健康号", () => {
    const files = [af("a", { success: 1, failed: 10 }), af("b", { success: 8, failed: 0 })];
    expect(servingFile(["a", "b"], files)).toBe("b");
  });

  it("跳过已禁用(待命/隔离/用户禁用)的号,即使它历史成功数很高", () => {
    const files = [af("a", { success: 99, failed: 0, disabled: true }), af("b", { success: 3, failed: 0 })];
    expect(servingFile(["a", "b"], files)).toBe("b");
  });

  it("无健康号(无流量 / 全失败)时返回 null(由调用方回退后端 active)", () => {
    const files = [af("a", { success: 0, failed: 0 }), af("b", { success: 0, failed: 5 })];
    expect(servingFile(["a", "b"], files)).toBeNull();
  });
});
