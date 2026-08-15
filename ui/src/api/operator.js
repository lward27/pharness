let operatorName = "console-operator";

export function setOperatorName(name) {
  if (typeof name === "string" && name.trim() !== "") operatorName = name.trim();
}

export function getOperatorName() {
  return operatorName;
}
