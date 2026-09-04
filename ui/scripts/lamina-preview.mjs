// Local, read-only visual review. This never connects to an operational API.
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { payload } from "../tests/fixtures/lamina.mjs";
const state=process.env.LAMINA_PREVIEW_STATE || "waiting";
const server=createServer((request,response)=>{
  response.setHeader("content-type","application/json");
  if(request.method!=="GET") { response.writeHead(405); response.end(JSON.stringify({error:"Read-only presentation fixture; actions are disabled."})); return; }
  const url=new URL(request.url,"http://127.0.0.1");
  response.end(JSON.stringify(payload(url.pathname,url.searchParams,state)));
});
server.listen(4789,"127.0.0.1",()=>{
  const ui=spawn("npm",["run","dev","--","--port","18442","--strictPort"],{stdio:"inherit",env:{...process.env,PHARNESS_API_PROXY:"http://127.0.0.1:4789",PHARNESS_API_PROXY_TOKEN:""}});
  console.log("Read-only Finance presentation fixture: http://127.0.0.1:18442/#/work-items/wi_market/overview");
  ui.once("exit",()=>server.close());
  for(const signal of ["SIGTERM","SIGINT"]) process.once(signal,()=>{ui.kill(signal);server.close();});
});
