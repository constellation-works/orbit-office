/**
 * Types shared by the server and the web UI. Types only: the web UI
 * imports this package with `import type`, so the browser never has to
 * resolve it (ARCHITECTURE.md, "Web app"). Office tool input and output
 * types join here, generated from the plugin's JSON Schemas, as tools are
 * built.
 */
export type {
  AcceptAllRequest,
  AcceptAllResult,
  AcceptRequest,
  AgentCliStatus,
  ApprovalDetail,
  ApprovalOk,
  ApproveRequest,
  ApproveResult,
  ApiCall,
  ConnectionInput,
  ConnectionSaved,
  ConnectionWrite,
  DismissRequest,
  FeatureName,
  GatewaySendRequest,
  GatewaySendResult,
  Health,
  EndpointKind,
  PrivateRouteBlock,
  PrivateRouteCheck,
  PrivateRouteCheckLine,
  PrivateRouteSaved,
  PrivateRouteStatus,
  PrivateRouteWrite,
  RouteCapability,
  ReviewItem,
  ReviewList,
  ScreenId,
  SendBackRequest,
  SettingsStatus,
  SettingsConnection,
  SettingsConnections,
  SettingsApiCalls,
  Status,
  Today,
  TodayItem,
  TodayNeed,
  TodayNeedGroup,
  TodayNeedKind,
  TodayResult,
  TodoItem,
  TodoList,
} from "./api.js";
export type { BootstrapPhase, CapturedSource, CapturePaste, SourceCitation, SourcePlan, SourceView } from "./api.js";
export type { SelectionInput, SelectionCount } from "./api.js";
