% createCMatCoup Creates either a coupled compliance matrix or a coupled stiffness matrix.
%
% Inputs:
%   Mat - Original compliance (6x6) or stiffness matrix.
%   MType - Type of matrix: 'C' for compliance, 'K' for stiffness.
%   varargin - Additional arguments depending on MType. 
%              For 'C': Transformation matrix for compliance.
%              For 'K': Two transformation matrices for stiffness.
%
% Outputs:
%   MatCoup - Coupled matrix, either compliance (6x6) or stiffness (12x12).
%
% Description:
%   This function calculates a coupled matrix (either compliance or stiffness) based on the provided matrix type (MType).
%   For compliance matrices (MType 'C'), it computes a coupled compliance matrix (6x6) by pre-multiplying and post-multiplying
%   the original compliance matrix with the transpose and the original transformation matrix, respectively.
%   For stiffness matrices (MType 'K'), it creates block matrices from the transformation matrices and the original 
%   stiffness matrix, resulting in a coupled stiffness matrix (12x12).
%   The transformation reflects changes in coordinates or other system properties.

function MatCoup = createMatCoup(Mat, MType, varargin)
    switch MType
        case 'C'
            % Extract transformation matrix for compliance matrix
            Tj = varargin{1};

            % Create coupled compliance matrix (6x6)  
            MatCoup = Tj * -Mat * Tj';

        case 'K'
            % Extract transformation matrices for stiffness matrix
            Ti_inv = varargin{1};
            Tj_inv = varargin{2};

            % Create block matrices of transformation matrices
            T_inv = createBlkDiag(Ti_inv, Tj_inv);

            % Create block matrix from 6x6 stiffness matrix
            kMatrixBlk = [-Mat, Mat; Mat, -Mat];

            % Create coupled stiffness matrix (12x12) 
            MatCoup = T_inv' * kMatrixBlk * T_inv;

        case 'KiKj'
            % Extract transformation matrices for stiffness matrix
            Ti_inv = varargin{1};
            Tj_inv = varargin{2};

            iMat = varargin{3};
            jMat = varargin{4};

            % Create block matrices of transformation matrices
            T_inv = createBlkDiag(Ti_inv, Tj_inv);

            % Create block matrix from 6x6 stiffness matrix
            kMatrixBlk = [-iMat, jMat; iMat, -jMat];

            % Create coupled stiffness matrix (12x12) 
            MatCoup = T_inv' * kMatrixBlk * T_inv;
            

        case 'KJCaseSix'
            % Extract transformation matrix for compliance matrix
            Tj = varargin{1};

            % Create coupled compliance matrix (6x6)  
            MatCoup = Tj * -Mat;
    end
end
