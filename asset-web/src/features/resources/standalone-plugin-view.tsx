import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import { Alert, Avatar, Box, Card, CardHeader, CircularProgress, Divider } from "@mui/material";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "react-router";
import { useGateway } from "@/application/ports/gateway-context";
import { PluginOutput } from "@/plugins/plugin-output";

export function StandalonePluginView() {
  const gateway = useGateway();
  const queryClient = useQueryClient();
  const { resourceId = "", actionId = "" } = useParams();
  const result = useQuery({
    queryKey: ["standalone-plugin-view", resourceId, actionId],
    queryFn: async () => {
      const resource = await gateway.findResource(resourceId);
      const action = resource.actions.find((candidate) => candidate.id === actionId);
      if (!action) throw new Error(`Action ${actionId} is not available.`);
      if (
        action.ui.confirmation &&
        !window.confirm(action.ui.confirmation.replaceAll("{name}", resource.name))
      ) {
        throw new Error(`Action ${actionId} was not confirmed.`);
      }
      return { resource, output: await gateway.executeResourceAction(resource, action.id) };
    },
    retry: false,
  });
  if (result.isPending) {
    return (
      <Box sx={{ display: "flex", justifyContent: "center", p: 4 }}>
        <CircularProgress />
      </Box>
    );
  }
  if (result.isError) {
    return (
      <Alert severity="error" sx={{ m: 2 }}>
        {result.error.message}
      </Alert>
    );
  }
  return (
    <Box component="main" sx={{ p: 2, maxWidth: 1280, mx: "auto" }}>
      <Card>
        <CardHeader
          avatar={
            <Avatar sx={{ bgcolor: "primary.main" }}>
              <StorageRoundedIcon />
            </Avatar>
          }
          title={result.data.resource.name}
          subheader={actionId}
        />
        <Divider />
        <PluginOutput
          output={result.data.output}
          resource={result.data.resource}
          onResourceChanged={() =>
            queryClient.invalidateQueries({
              queryKey: ["standalone-plugin-view", resourceId, actionId],
            })
          }
        />
      </Card>
    </Box>
  );
}
