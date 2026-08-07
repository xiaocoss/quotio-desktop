import { describe, it, expect } from "vitest";
import {
  authFileKey,
  findAuthFileByName,
  isAuthFailureMessage,
  matchAuthFile,
  servingFile,
} from "./format";
import type { AccountQuota, AuthFile } from "../types";

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

function quota(partial: Partial<AccountQuota>): AccountQuota {
  return {
    provider_id: "codex",
    account_label: "a@example.com",
    account_key: "a@example.com",
    is_forbidden: false,
    status_message: null,
    models: [],
    ...partial,
  } as AccountQuota;
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

  it("调度顺序(磁盘原始大小写)与代理账号名(全小写)大小写不一致时仍能匹配", () => {
    // 后端顺序来自本地 auth 目录(保留大小写),authFiles 来自代理 /auth-files(小写化)。
    const files = [af("codex-martilloolivia.json", { success: 7, failed: 0 })];
    expect(servingFile(["codex-MartilloOlivia.json"], files)).toBe("codex-MartilloOlivia.json");
  });
});

describe("authFileKey / findAuthFileByName", () => {
  it("大小写与首尾空白都归一化", () => {
    expect(authFileKey(" Codex-A.JSON ")).toBe("codex-a.json");
  });

  it("按名字查账号忽略大小写", () => {
    const files = [af("codex-Alice.json", {}), af("codex-bob.json", {})];
    expect(findAuthFileByName(files, "CODEX-ALICE.JSON")?.name).toBe("codex-Alice.json");
    expect(findAuthFileByName(files, "codex-missing.json")).toBeNull();
  });
});

describe("isAuthFailureMessage", () => {
  it("认所有服务商的鉴权失败哨兵(与后端 AccountQuota::is_auth_failure 同集合)", () => {
    for (const message of ["auth_failed", "需要重新授权", "需要重新登录", "密钥无效"]) {
      expect(isAuthFailureMessage(message)).toBe(true);
    }
  });

  it("额度耗尽 / 无消息不算鉴权失败", () => {
    expect(isAuthFailureMessage(null)).toBe(false);
    expect(isAuthFailureMessage(undefined)).toBe(false);
    expect(isAuthFailureMessage("plan: pro | until: 2026-01-01")).toBe(false);
    expect(isAuthFailureMessage("pay_as_you_go")).toBe(false);
  });
});

describe("matchAuthFile", () => {
  it("provider 为空且文件名无前缀时不再匹配到任意服务商", () => {
    // "任意字符串".includes("") 恒为 true —— 一个没有 provider 的文件曾因此成为
    // 每个服务商每个账号的候选,只要邮箱撞上就会错配。
    const files = [af("mystery.json", { provider: "", email: "a@example.com" })];
    expect(matchAuthFile(quota({ provider_id: "codex" }), files)).toBeNull();
  });

  it("provider 为空时退回文件名前缀定界,不跨服务商错配", () => {
    const files = [af("trae-a.json", { provider: "", email: "a@example.com" })];
    // 同一邮箱同时存在于 codex 与 trae 时,不能把 codex 的额度卡挂到 trae 的文件上。
    expect(matchAuthFile(quota({ provider_id: "codex" }), files)).toBeNull();
    expect(matchAuthFile(quota({ provider_id: "trae" }), files)?.name).toBe("trae-a.json");
  });

  it("gemini-cli 账号仍能匹配 provider 为 gemini 的文件(前缀包含关系)", () => {
    const files = [af("gemini-a.json", { provider: "gemini", email: "a@example.com" })];
    const matched = matchAuthFile(
      quota({ provider_id: "gemini-cli", account_label: "a@example.com" }),
      files,
    );
    expect(matched?.name).toBe("gemini-a.json");
  });
});
