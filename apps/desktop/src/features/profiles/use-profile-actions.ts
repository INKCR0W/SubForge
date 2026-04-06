import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  createProfile,
  deleteProfile,
  rotateProfileExportToken,
  updateProfile,
} from "../../lib/api";
import {
  patchProfileItem,
  removeProfileItem,
  upsertProfileItem,
} from "../../lib/query-cache";
import { queryKeys } from "../../lib/query-keys";
import type { ToastMessage } from "../../stores/core-ui-store";
import type { ProfileListResponse } from "../../types/core";
import { type InlineActionState } from "../../components/inline-action-feedback";
import { type ProfileFormMode } from "./constants";
import { formatTimestamp } from "./utils";

type UseProfileActionsOptions = {
  addToast: (toast: Omit<ToastMessage, "id">) => string;
  eventDrivenSyncEnabled: boolean;
  formMode: ProfileFormMode;
  onResetForm: () => void;
};

type UpdateProfileInput = {
  profileId: string;
  name: string;
  description?: string | null;
  sourceIds: string[];
  routingTemplateSourceId?: string | null;
};

export function useProfileActions({
  addToast,
  eventDrivenSyncEnabled,
  formMode,
  onResetForm,
}: UseProfileActionsOptions) {
  const queryClient = useQueryClient();
  const [activeProfileId, setActiveProfileId] = useState<string | null>(null);
  const [inlineAction, setInlineAction] = useState<InlineActionState>({
    phase: "idle",
    title: "",
    description: "",
  });

  const createMutation = useMutation({
    mutationFn: createProfile,
    onMutate: async (input) => {
      setInlineAction({
        phase: "loading",
        title: "正在创建 Profile",
        description: `已提交 ${input.name}，等待 Core 确认。`,
      });
      await queryClient.cancelQueries({ queryKey: queryKeys.profiles.all });
      const previousProfiles = queryClient.getQueryData<ProfileListResponse>(
        queryKeys.profiles.all,
      );
      const optimisticProfileId = `optimistic-profile-${Date.now()}`;
      const now = new Date().toISOString();

      queryClient.setQueryData<ProfileListResponse>(queryKeys.profiles.all, (current) =>
        upsertProfileItem(current, {
          profile: {
            id: optimisticProfileId,
            name: input.name,
            description: input.description ?? null,
            routing_template_source_id: input.routingTemplateSourceId ?? null,
            created_at: now,
            updated_at: now,
          },
          source_ids: input.sourceIds,
          export_token: null,
        }),
      );

      return { previousProfiles, optimisticProfileId };
    },
    onSuccess: (payload, _input, context) => {
      queryClient.setQueryData<ProfileListResponse>(queryKeys.profiles.all, (current) =>
        upsertProfileItem(
          removeProfileItem(current, context?.optimisticProfileId ?? ""),
          payload.profile,
        ),
      );
      addToast({
        title: "Profile 创建成功",
        description: payload.profile.profile.name,
        variant: "default",
      });
      setInlineAction({
        phase: "success",
        title: "Profile 创建成功",
        description: `${payload.profile.profile.name} 已可用于导出。`,
      });
      onResetForm();
    },
    onError: (error, _input, context) => {
      if (context) {
        queryClient.setQueryData(queryKeys.profiles.all, context.previousProfiles);
      }
      addToast({
        title: "Profile 创建失败",
        description: error instanceof Error ? error.message : "未知错误",
        variant: "error",
      });
      setInlineAction({
        phase: "error",
        title: "Profile 创建失败",
        description: error instanceof Error ? error.message : "未知错误",
      });
    },
    onSettled: () => {
      if (!eventDrivenSyncEnabled) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      }
    },
  });

  const updateMutation = useMutation({
    mutationFn: (input: UpdateProfileInput) =>
      updateProfile(input.profileId, {
        name: input.name,
        description: input.description,
        sourceIds: input.sourceIds,
        routingTemplateSourceId: input.routingTemplateSourceId,
      }),
    onMutate: async (input) => {
      setInlineAction({
        phase: "loading",
        title: "正在保存 Profile",
        description: `正在更新 ${input.name}。`,
      });
      await queryClient.cancelQueries({ queryKey: queryKeys.profiles.all });
      const previousProfiles = queryClient.getQueryData<ProfileListResponse>(
        queryKeys.profiles.all,
      );
      queryClient.setQueryData<ProfileListResponse | undefined>(queryKeys.profiles.all, (current) =>
        patchProfileItem(current, input.profileId, {
          name: input.name,
          description: input.description ?? null,
          routingTemplateSourceId: input.routingTemplateSourceId ?? null,
          sourceIds: input.sourceIds,
          updatedAt: new Date().toISOString(),
        }),
      );
      return { previousProfiles };
    },
    onSuccess: (payload) => {
      queryClient.setQueryData<ProfileListResponse>(queryKeys.profiles.all, (current) =>
        upsertProfileItem(current, payload.profile),
      );
      addToast({
        title: "Profile 更新成功",
        description: payload.profile.profile.name,
        variant: "default",
      });
      setInlineAction({
        phase: "success",
        title: "Profile 保存成功",
        description: `${payload.profile.profile.name} 已同步最新配置。`,
      });
    },
    onError: (error, _input, context) => {
      if (context) {
        queryClient.setQueryData(queryKeys.profiles.all, context.previousProfiles);
      }
      addToast({
        title: "Profile 更新失败",
        description: error instanceof Error ? error.message : "未知错误",
        variant: "error",
      });
      setInlineAction({
        phase: "error",
        title: "Profile 保存失败",
        description: error instanceof Error ? error.message : "未知错误",
      });
    },
    onSettled: () => {
      if (!eventDrivenSyncEnabled) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      }
      setActiveProfileId(null);
    },
  });

  const rotateMutation = useMutation({
    mutationFn: rotateProfileExportToken,
    onMutate: () => {
      setInlineAction({
        phase: "loading",
        title: "正在轮换 Token",
        description: "请求已提交，等待 Core 返回新的导出 token。",
      });
    },
    onSuccess: (payload) => {
      queryClient.setQueryData<ProfileListResponse | undefined>(
        queryKeys.profiles.all,
        (current) =>
          patchProfileItem(current, payload.profile_id, {
            exportToken: payload.token,
            updatedAt: new Date().toISOString(),
          }),
      );
      addToast({
        title: "导出 Token 已轮换",
        description: `旧链接将在 ${formatTimestamp(payload.previous_token_expires_at)} 失效。`,
        variant: "warning",
      });
      setInlineAction({
        phase: "success",
        title: "Token 轮换成功",
        description: `旧 token 将在 ${formatTimestamp(payload.previous_token_expires_at)} 失效。`,
      });
    },
    onError: (error) => {
      addToast({
        title: "Token 轮换失败",
        description: error instanceof Error ? error.message : "未知错误",
        variant: "error",
      });
      setInlineAction({
        phase: "error",
        title: "Token 轮换失败",
        description: error instanceof Error ? error.message : "未知错误",
      });
    },
    onSettled: () => {
      if (!eventDrivenSyncEnabled) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      }
      setActiveProfileId(null);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: deleteProfile,
    onMutate: async (profileId) => {
      setInlineAction({
        phase: "loading",
        title: "正在删除 Profile",
        description: `Profile ${profileId} 删除请求已提交。`,
      });
      await queryClient.cancelQueries({ queryKey: queryKeys.profiles.all });
      const previousProfiles = queryClient.getQueryData<ProfileListResponse>(
        queryKeys.profiles.all,
      );
      queryClient.setQueryData<ProfileListResponse | undefined>(queryKeys.profiles.all, (current) =>
        removeProfileItem(current, profileId),
      );
      return { previousProfiles };
    },
    onSuccess: () => {
      addToast({
        title: "Profile 已删除",
        description: "关联导出地址已失效。",
        variant: "warning",
      });
      setInlineAction({
        phase: "success",
        title: "Profile 删除成功",
        description: "关联导出地址已失效。",
      });
      if (formMode === "edit") {
        onResetForm();
      }
    },
    onError: (error, _input, context) => {
      if (context) {
        queryClient.setQueryData(queryKeys.profiles.all, context.previousProfiles);
      }
      addToast({
        title: "Profile 删除失败",
        description: error instanceof Error ? error.message : "未知错误",
        variant: "error",
      });
      setInlineAction({
        phase: "error",
        title: "Profile 删除失败",
        description: error instanceof Error ? error.message : "未知错误",
      });
    },
    onSettled: () => {
      if (!eventDrivenSyncEnabled) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      }
      setActiveProfileId(null);
    },
  });

  return {
    activeProfileId,
    inlineAction,
    setActiveProfileId,
    createMutation,
    updateMutation,
    rotateMutation,
    deleteMutation,
  };
}
