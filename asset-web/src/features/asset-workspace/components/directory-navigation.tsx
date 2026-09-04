import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import { Breadcrumbs, Link } from "@mui/material";
import { breadcrumbs } from "@/domain/directory-path";

/** Host-owned path navigation rendered in the primary Asset Hub header. */
export function DirectoryBreadcrumbs({
  path,
  onNavigate,
}: {
  path: string;
  onNavigate: (path: string) => void;
}) {
  const crumbs = breadcrumbs(path);
  return (
    <Breadcrumbs
      separator={<ChevronRightIcon fontSize="small" />}
      aria-label="Directory path"
      sx={{
        minWidth: 0,
        flex: 1,
        flexBasis: { xs: "100%", md: "auto" },
        order: { xs: 3, md: 0 },
        overflow: "hidden",
      }}
    >
      {crumbs.map((crumb) => (
        <Link
          key={crumb.path || "root"}
          component="button"
          underline="hover"
          onClick={() => onNavigate(crumb.path)}
        >
          {crumb.label}
        </Link>
      ))}
    </Breadcrumbs>
  );
}
