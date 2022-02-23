from typing import Dict, TypedDict, List
from flask import Flask, request
import jsonschema

from permissions.api import AdhocProject

app = Flask(__name__)


CREATE_PROJECT_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://smbdrop.cloud-erc.fz-juelich.de/create-project-request.schema.json",
    "title": "CreateProjectRequest",
    "description": "The JSON payload for the POST request to create a new project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Short name of the project",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the project",
        },
        "members": {
            "type": "array",
            "items": {
                "type": "string",
            },
            "minItems": 1,
            "uniqueItems": True,
            "description": "Usernames of the members of this ad-hoc group",
        },
    },
    "required": ["name", "owner", "members"],
}


class CreateProjectRequest(TypedDict):
    name: str
    owner: str
    members: List[str]


@app.route("/project/", methods=["POST"])
def create_project() -> Dict:
    request_data: CreateProjectRequest = request.json  # type:ignore
    jsonschema.validate(schema=CREATE_PROJECT_REQUEST_SCHEMA, instance=request_data)
    project = AdhocProject.create(
        name=request_data['name'],
        owner=request_data['owner'],
        members=request_data['members']
    )
    return {}  # FIXME: what would you like as a return type? AdhocProject.serialize -> TypedDict?


@app.route("/project/", methods=["GET"])
def list_projects() -> Dict:
    project_names = AdhocProject.list_all()
    return {
        'project_names': project_names,
    }


@app.route("/project/<name>/", methods=["GET"])
def project_details(name) -> Dict:
    # TODO: catch exceptions?
    project_names = AdhocProject.list_all()
    projects: List[Dict] = [
        {}  # TODO: what details are needed here?
        for project_name in project_names
    ]
    return {
        'projects': projects,
    }


@app.route("/project/<project_name>/datasets/", methods=["GET"])
def list_datasets(project_name) -> Dict:
    # TODO: catch exceptions?
    p = AdhocProject(project_name)

    return {
        "project": project_name,
        "datasets": [
            {"name": ds.name, "immutable": ds.immutable}
            for ds in p.datasets
        ]
    }


CREATE_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://smbdrop.cloud-erc.fz-juelich.de/create-dataset-request.schema.json",
    "title": "CreateDatasetRequest",
    "description": "The JSON payload for the POST request to create a new dataset in a project",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset, which will be used as directory name",
        },
        "owner": {
            "type": "string",
            "description": "Username of the owner of the dataset",
        },
    },
    "required": ["name", "owner"],
}


class CreateDatasetRequest(TypedDict):
    name: str
    owner: str


@app.route("/project/<project_name>/datasets/", methods=["POST"])
def create_dataset(project_name) -> Dict:
    request_data: CreateDatasetRequest = request.json  # type:ignore
    jsonschema.validate(
        schema=CREATE_DATASET_REQUEST_SCHEMA, instance=request_data
    )
    # TODO: catch exceptions?
    p = AdhocProject(project_name)
    p.add_dataset(
        name=request_data["name"],
        user=request_data["owner"]
    )
    return {}  # what should be returned here?


SEAL_DATASET_REQUEST_SCHEMA = {
    "$schema": "http://json-schema.org/draft-07/schema#",
    "$id": "http://smbdrop.cloud-erc.fz-juelich.de/seal-dataset-request.schema.json",
    "title": "SealDatasetRequest",
    "description": "The JSON payload for the POST request to seal a dataset",
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Name of the dataset",
        },
    },
    "required": ["name"],
}


class SealDatasetRequest(TypedDict):
    name: str


# NOTE: not using data set name in path, as it can contain slashes (right?) so
# we are using a json payload for this request, which may seem a bit like
# overkill....
@app.route("/project/<project_name>/datasets/seal/", methods=["POST"])
def seal_dataset(project_name):
    request_data: CreateDatasetRequest = request.json  # type:ignore
    jsonschema.validate(
        schema=SEAL_DATASET_REQUEST_SCHEMA, instance=request_data
    )
    # TODO: catch exceptions?
    p = AdhocProject(project_name)
    p.seal_dataset(request_data["name"])
    return {}  # what should be returned here?
