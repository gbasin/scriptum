export interface Section {
  id: string;
  parentId: string | null;
  heading: string;
  level: number;
  startLine: number;
  endLine: number;
}
