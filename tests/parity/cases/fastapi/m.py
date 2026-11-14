from fastapi import FastAPI, APIRouter, Depends
app = FastAPI()
router = APIRouter()
@app.get("/")
def root():
    return {}
@router.post("/x")
async def create():
    return {}
def get_db():
    return 1
@app.get("/y")
def y(db=Depends(get_db)):
    return db
