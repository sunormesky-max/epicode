# TetraMem v14.2.1 Upload & Deploy Script
# Run from Windows PowerShell

param([string]$DeepseekKey = "")

if (-not $DeepseekKey) {
    Write-Host "Usage: .\upload_deploy.ps1 <DEEPSEEK_API_KEY>"
    exit 1
}

$SERVER = "root@111.231.24.199"
$SSH_KEY = "$env:USERPROFILE\.ssh\id_ed25519"
$REMOTE_DIR = "/opt/tetramem"

Write-Host "=== TetraMem Upload & Deploy ==="
Write-Host ""

# Step 1: Upload deploy package
Write-Host "[1/3] Uploading deploy scripts..."
ssh -i $SSH_KEY -o StrictHostKeyChecking=no $SERVER "mkdir -p $REMOTE_DIR/deploy"
scp -i $SSH_KEY deploy/*.sh "${SERVER}:${REMOTE_DIR}/deploy/"

# Step 2: Upload ONNX model (415MB, skip if exists)
Write-Host "[2/3] Uploading ONNX model (415MB, may take a few minutes)..."
$MODEL_EXISTS = ssh -i $SSH_KEY $SERVER "test -f ${REMOTE_DIR}/models/model.onnx && echo YES || echo NO"
if ($MODEL_EXISTS -match "NO") {
    scp -i $SSH_KEY models/model.onnx "${SERVER}:${REMOTE_DIR}/models/model.onnx"
    scp -i $SSH_KEY models/tokenizer.json "${SERVER}:${REMOTE_DIR}/models/tokenizer.json"
    Write-Host "Model uploaded."
} else {
    Write-Host "Model already exists on server, skipping."
}

# Step 3: Run setup
Write-Host "[3/3] Running remote setup..."
ssh -i $SSH_KEY -t $SERVER "cd ${REMOTE_DIR}/deploy && bash setup.sh `"$DeepseekKey`""

Write-Host ""
Write-Host "=== Done ==="
