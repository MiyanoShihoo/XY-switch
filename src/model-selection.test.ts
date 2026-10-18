import { describe, expect, it } from "vitest";
import { addDiscoveredModelMapping, mergeDiscoveredModelMappings, reorderModelMappings } from "./lib/model-selection";
import type { DiscoveredModel } from "./lib/model-service";
import type { ProviderModelMapping } from "./types/provider";

const existing: ProviderModelMapping[] = [
  { model: "model-a", displayName: "用户名称", contextWindow: 128000 },
];

const discovered: DiscoveredModel[] = [
  { id: " MODEL-A ", name: "探索名称" },
  { id: " model-b ", name: "模型 B" },
  { id: "   ", name: "空模型" },
  { id: "MODEL-B", name: "重复模型 B" },
];

describe("探索模型映射", () => {
  it("单个导入会规范化 ID 且按 ID 不区分大小写去重", () => {
    expect(addDiscoveredModelMapping(existing, discovered[0])).toEqual(existing);
    expect(addDiscoveredModelMapping(existing, discovered[1])).toEqual([
      existing[0],
      { model: "model-b", displayName: "模型 B", contextWindow: null },
    ]);
  });

  it("全部导入会忽略空 ID 和重复项，并保留已有用户配置", () => {
    expect(mergeDiscoveredModelMappings(existing, discovered)).toEqual([
      { model: "model-a", displayName: "用户名称", contextWindow: 128000 },
      { model: "model-b", displayName: "模型 B", contextWindow: null },
    ]);
  });

  it("不修改调用方传入的数组和对象", () => {
    const result = mergeDiscoveredModelMappings(existing, discovered);
    expect(result).not.toBe(existing);
    expect(result[0]).toBe(existing[0]);
    expect(existing).toHaveLength(1);
  });
});


describe("模型映射排序", () => {
  const mappings: ProviderModelMapping[] = [
    { model: "duplicate", displayName: "第一个", contextWindow: 1000 },
    { model: "", displayName: "空模型", contextWindow: null },
    { model: "duplicate", displayName: "第二个", contextWindow: 2000 },
  ];

  it("按索引移动并完整保留字段、重复模型和空模型", () => {
    expect(reorderModelMappings(mappings, 2, 0)).toEqual([mappings[2], mappings[0], mappings[1]]);
    expect(mappings.map((mapping) => mapping.displayName)).toEqual(["第一个", "空模型", "第二个"]);
  });

  it("无效索引或原位移动返回顺序不变的新数组", () => {
    for (const [fromIndex, toIndex] of [[-1, 0], [0, 3], [0, 0], [0.5, 1]]) {
      const result = reorderModelMappings(mappings, fromIndex, toIndex);
      expect(result).toEqual(mappings);
      expect(result).not.toBe(mappings);
    }
  });
});
