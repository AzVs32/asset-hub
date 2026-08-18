import { createTheme } from "@mui/material/styles";

export const theme = createTheme({
  cssVariables: true,
  palette: {
    mode: "light",
    primary: {
      main: "#5b5bd6",
      light: "#818cf8",
      dark: "#4338ca",
      contrastText: "#ffffff",
    },
    secondary: {
      main: "#db2777",
      light: "#f472b6",
      dark: "#9d174d",
    },
    info: { main: "#0284c7" },
    success: { main: "#16a34a" },
    warning: { main: "#d97706" },
    background: {
      default: "#f2f5fb",
      paper: "#ffffff",
    },
    text: {
      primary: "#182033",
      secondary: "#64748b",
    },
    divider: "#dfe5ef",
  },
  shape: { borderRadius: 12 },
  typography: {
    fontFamily:
      'Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    h6: { fontWeight: 750, letterSpacing: "-0.025em" },
    subtitle1: { fontWeight: 700 },
    button: { fontWeight: 700, textTransform: "none" },
  },
  components: {
    MuiCssBaseline: {
      styleOverrides: {
        body: {
          minWidth: 320,
          minHeight: "100vh",
          background:
            "radial-gradient(circle at 12% -8%, rgba(99, 102, 241, 0.18), transparent 30rem), radial-gradient(circle at 92% 12%, rgba(236, 72, 153, 0.1), transparent 24rem), #f2f5fb",
        },
        "::selection": {
          color: "#312e81",
          backgroundColor: "#c7d2fe",
        },
      },
    },
    MuiAppBar: {
      defaultProps: { elevation: 0 },
      styleOverrides: {
        root: {
          color: "#182033",
          background: "rgba(255, 255, 255, 0.9)",
          borderBottom: "1px solid rgba(203, 213, 225, 0.8)",
          boxShadow: "0 14px 35px -28px rgba(15, 23, 42, 0.6)",
          backdropFilter: "blur(18px)",
        },
      },
    },
    MuiPaper: {
      defaultProps: { elevation: 0 },
      styleOverrides: {
        root: {
          border: "1px solid #dfe5ef",
        },
      },
    },
    MuiCard: {
      styleOverrides: {
        root: {
          backgroundImage: "linear-gradient(155deg, #ffffff 35%, #fafbff 100%)",
          boxShadow: "0 20px 55px -36px rgba(30, 41, 59, 0.7)",
        },
      },
    },
    MuiDialog: {
      styleOverrides: {
        paper: {
          boxShadow: "0 30px 80px -30px rgba(15, 23, 42, 0.65)",
        },
      },
    },
    MuiButton: {
      defaultProps: { disableElevation: true },
      styleOverrides: {
        root: {
          borderRadius: 10,
        },
        contained: {
          "&.Mui-disabled": {
            color: "#475569",
            backgroundColor: "#d9e2ef",
            backgroundImage: "none",
            border: "1px solid #bac7d8",
            boxShadow: "none",
          },
        },
        containedPrimary: {
          backgroundImage: "linear-gradient(135deg, #6366f1, #4f46e5)",
          boxShadow: "0 10px 24px -12px rgba(79, 70, 229, 0.9)",
          "&:hover": {
            backgroundImage: "linear-gradient(135deg, #5558e8, #4338ca)",
          },
        },
      },
    },
    MuiAvatar: {
      styleOverrides: {
        root: {
          color: "#4338ca",
          background: "linear-gradient(145deg, #eef2ff, #e0e7ff)",
          boxShadow: "inset 0 0 0 1px rgba(99, 102, 241, 0.12)",
        },
      },
    },
    MuiOutlinedInput: {
      styleOverrides: {
        root: {
          backgroundColor: "rgba(255, 255, 255, 0.8)",
          "&:hover .MuiOutlinedInput-notchedOutline": {
            borderColor: "#a5b4fc",
          },
          "&.Mui-focused": {
            boxShadow: "0 0 0 4px rgba(99, 102, 241, 0.1)",
          },
        },
      },
    },
    MuiListItemButton: {
      styleOverrides: {
        root: {
          margin: "3px 8px",
          borderRadius: 12,
          "&:hover": {
            backgroundColor: "#f5f3ff",
          },
          "&.Mui-selected": {
            color: "#3730a3",
            backgroundColor: "#eef2ff",
            boxShadow: "inset 3px 0 #6366f1",
          },
          "&.Mui-selected:hover": {
            backgroundColor: "#e0e7ff",
          },
        },
      },
    },
    MuiChip: {
      styleOverrides: {
        root: { fontWeight: 700 },
      },
    },
    MuiLinearProgress: {
      styleOverrides: {
        root: {
          height: 8,
          borderRadius: 999,
        },
        bar: { borderRadius: 999 },
      },
    },
  },
});
